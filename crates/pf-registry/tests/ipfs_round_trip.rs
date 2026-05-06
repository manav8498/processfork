// SPDX-License-Identifier: MIT
//! End-to-end IPFS registry round-trip against a wiremock server that
//! speaks just enough of the Kubo `/api/v0` HTTP RPC to serve
//! ProcessFork artifacts.
//!
//! Confirms the v1.0.2 ipfs:// adapter actually:
//!   1. POSTs `/api/v0/add` (multipart) for manifest+sig+each blob,
//!   2. POSTs `/api/v0/object/new?arg=unixfs-dir` to get a root,
//!   3. POSTs `/api/v0/object/patch/add-link?arg=...` for each entry,
//!   4. POSTs `/api/v0/pin/add?arg=...` to pin the result,
//!   5. on pull: POSTs `/api/v0/ls?arg=...` then `/api/v0/cat?arg=...`,
//!      verifies signature + every blob digest.
//!
//! The wiremock server uses **content-addressed fake CIDs** (sha256 of
//! the bytes, prefixed with a 'fake-' tag) so push and pull line up
//! exactly. Real Kubo would emit valid CIDv1 base32 strings; we don't
//! need that fidelity to validate our wire format.

#![cfg(feature = "ipfs-live")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pf_core::cas::{BlobStore, MemBlobStore};
use pf_core::digest::Digest256;
use pf_core::fixture::{
    FixtureCacheCapture, FixtureEffectsCapture, FixtureModelCapture, FixtureSpec,
    FixtureTraceCapture, FixtureWorldCapture,
};
use pf_core::manifest::AgentInfo;
use pf_core::snapshot::Snapshotter;
use pf_core::store::PfStore;
use pf_registry::{ImageRef, IpfsRegistry, Registry};
use serde_json::{Value, json};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn snapshotter() -> Snapshotter {
    let agent = AgentInfo {
        kind: "test".into(),
        version: "0".into(),
        fingerprint: "ipfs-test".into(),
    };
    let spec = FixtureSpec::default();
    Snapshotter::new(
        agent,
        Arc::new(FixtureModelCapture(spec.clone())),
        Arc::new(FixtureCacheCapture(spec.clone())),
        Arc::new(FixtureWorldCapture(spec.clone())),
        Arc::new(FixtureEffectsCapture(spec.clone())),
        Arc::new(FixtureTraceCapture(spec)),
    )
}

#[derive(Clone, Default)]
struct FakeIpfs {
    /// CID → bytes (for individual files).
    blobs: BlobMap,
    /// dir CID → list of (entry_name, child_cid).
    dirs: DirMap,
}

type BlobMap = Arc<Mutex<HashMap<String, Vec<u8>>>>;
type DirMap = Arc<Mutex<HashMap<String, Vec<(String, String)>>>>;

fn fake_cid(bytes: &[u8]) -> String {
    format!("fake-{}", Digest256::of(bytes).hex())
}

/// Pull the multipart payload's file body out. Multipart bodies are
/// `--<boundary>\r\nContent-Disposition: form-data; name="file";
/// filename="..."\r\n...`. We don't parse multipart properly — we
/// just slice between the FIRST blank line and the trailing boundary.
fn extract_multipart_body(req: &Request) -> Vec<u8> {
    let body = &req.body;
    // Find the first \r\n\r\n separator.
    let sep = b"\r\n\r\n";
    let Some(start) = body.windows(sep.len()).position(|w| w == sep) else {
        return Vec::new();
    };
    let after_headers = &body[start + sep.len()..];
    // Trailing boundary is "\r\n--<boundary>--\r\n"; trim by walking
    // back to the last \r\n before "--".
    let trail = b"\r\n--";
    let end = after_headers
        .windows(trail.len())
        .rposition(|w| w == trail)
        .unwrap_or(after_headers.len());
    after_headers[..end].to_vec()
}

struct AddResponder(FakeIpfs);
impl Respond for AddResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let bytes = extract_multipart_body(req);
        let cid = fake_cid(&bytes);
        self.0.blobs.lock().unwrap().insert(cid.clone(), bytes);
        ResponseTemplate::new(200).set_body_json(json!({"Hash": cid, "Name": "f", "Size": "0"}))
    }
}

struct NewDirResponder(FakeIpfs);
impl Respond for NewDirResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let cid = format!("fake-dir-{}", uuid_like());
        self.0.dirs.lock().unwrap().insert(cid.clone(), Vec::new());
        ResponseTemplate::new(200).set_body_json(json!({"Hash": cid, "Links": []}))
    }
}

struct AddLinkResponder(FakeIpfs);
impl Respond for AddLinkResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        // arg=<dir>&arg=<name>&arg=<cid>
        let args: Vec<String> = req
            .url
            .query_pairs()
            .filter(|(k, _)| k == "arg")
            .map(|(_, v)| v.to_string())
            .collect();
        if args.len() != 3 {
            return ResponseTemplate::new(400);
        }
        let (dir, name, target) = (args[0].clone(), args[1].clone(), args[2].clone());
        let mut dirs = self.0.dirs.lock().unwrap();
        let entries = dirs.entry(dir.clone()).or_default().clone();
        let mut new_entries = entries.clone();
        new_entries.push((name, target));
        // New dir CID — derive deterministically from the entries so
        // this is reproducible across test runs but unique per dir.
        let mut h = Vec::new();
        for (n, c) in &new_entries {
            h.extend_from_slice(n.as_bytes());
            h.extend_from_slice(c.as_bytes());
        }
        let new_cid = fake_cid(&h);
        dirs.insert(new_cid.clone(), new_entries);
        ResponseTemplate::new(200).set_body_json(json!({"Hash": new_cid, "Links": []}))
    }
}

struct PinResponder;
impl Respond for PinResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"Pins": []}))
    }
}

struct LsResponder(FakeIpfs);
impl Respond for LsResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let cid = req
            .url
            .query_pairs()
            .find(|(k, _)| k == "arg")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        let dirs = self.0.dirs.lock().unwrap();
        let Some(entries) = dirs.get(&cid) else {
            return ResponseTemplate::new(500)
                .set_body_json(json!({"Message": "not a directory", "Code": 0, "Type": "error"}));
        };
        let links: Vec<Value> = entries
            .iter()
            .map(|(n, c)| json!({"Name": n, "Hash": c, "Size": 0, "Type": 2}))
            .collect();
        ResponseTemplate::new(200)
            .set_body_json(json!({"Objects": [{"Hash": cid, "Links": links}]}))
    }
}

struct CatResponder(FakeIpfs);
impl Respond for CatResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let cid = req
            .url
            .query_pairs()
            .find(|(k, _)| k == "arg")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        match self.0.blobs.lock().unwrap().get(&cid) {
            Some(b) => ResponseTemplate::new(200).set_body_bytes(b.clone()),
            None => ResponseTemplate::new(404),
        }
    }
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{n:x}")
}

#[tokio::test]
async fn ipfs_full_round_trip_via_wiremock() {
    let server = MockServer::start().await;
    let state = FakeIpfs::default();

    Mock::given(method("POST"))
        .and(path("/api/v0/add"))
        .respond_with(AddResponder(state.clone()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v0/object/new"))
        .respond_with(NewDirResponder(state.clone()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v0/object/patch/add-link"))
        .respond_with(AddLinkResponder(state.clone()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v0/pin/add"))
        .respond_with(PinResponder)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v0/ls"))
        .respond_with(LsResponder(state.clone()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v0/cat"))
        .respond_with(CatResponder(state.clone()))
        .mount(&server)
        .await;

    // Snapshot.
    let local_dir = TempDir::new().unwrap();
    let local_store = PfStore::open(local_dir.path()).unwrap();
    let cid = snapshotter().snapshot(&local_store, vec![]).unwrap();
    let manifest = local_store.get_manifest(&cid).unwrap();

    let reg = IpfsRegistry::new(server.uri()).with_sign_key("ipfs-test-key");

    // Push: hint CID is empty (we don't know the resulting dir CID).
    let push_target = ImageRef::Ipfs { cid: String::new() };
    reg.push(&push_target, &manifest, local_store.blobs())
        .await
        .expect("IPFS push");

    // The fake IPFS state now contains exactly one final dir CID — find it
    // (it's the LAST one created via the chain of add-link calls).
    let final_dir_cid = {
        let dirs = state.dirs.lock().unwrap();
        // The "biggest" dir (with manifest+sig+all blobs) is the one
        // that has manifest.json among its entries.
        dirs.iter()
            .find(|(_, entries)| entries.iter().any(|(n, _)| n == "manifest.json"))
            .map(|(k, _)| k.clone())
            .expect("at least one dir with manifest.json should exist")
    };

    // Pull from the actual final dir CID and confirm round-trip.
    let pull_target = ImageRef::Ipfs { cid: final_dir_cid };
    let pulled = reg.pull(&pull_target).await.expect("IPFS pull");
    assert_eq!(pulled.manifest.schema_version, 1);

    let mem = MemBlobStore::new();
    for (digest, bytes) in &pulled.blobs {
        let put_d = mem.put(bytes).unwrap();
        assert_eq!(&put_d, digest, "pulled blob hashes to its digest");
    }
    let mem_cid = mem
        .put(&serde_json::to_vec(&pulled.manifest).unwrap())
        .unwrap();
    assert_eq!(mem_cid, cid, "manifest CID round-trips through IPFS");

    // exists() against the same CID returns true.
    assert!(reg.exists(&pull_target).await.unwrap());
}

#[tokio::test]
async fn ipfs_pull_missing_cid_returns_backend_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v0/ls"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_json(json!({"Message": "not pinned", "Code": 0, "Type": "error"})),
        )
        .mount(&server)
        .await;
    let reg = IpfsRegistry::new(server.uri());
    let r = reg
        .pull(&ImageRef::Ipfs { cid: "Qm0".into() })
        .await
        .unwrap_err();
    let msg = format!("{r}");
    assert!(
        msg.contains("500") || msg.contains("ls"),
        "expected ls error, got: {msg}"
    );
}
