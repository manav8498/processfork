// SPDX-License-Identifier: MIT
//! End-to-end OCI Distribution Spec round-trip against a wiremock
//! server that implements just enough of the v2 protocol to serve
//! ProcessFork artifacts.
//!
//! Confirms the v1.0.2 oci:// adapter actually:
//!   1. POSTs `/v2/<repo>/blobs/uploads/` and PUTs `?digest=...`,
//!   2. PUTs `/v2/<repo>/manifests/<tag>` once all blobs are uploaded,
//!   3. GETs the manifest then walks layers + config blob,
//!   4. verifies the signature layer + every blob digest on pull.
//!
//! Gated by `--features oci-live` (default).

#![cfg(feature = "oci-live")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pf_core::cas::{BlobStore, MemBlobStore};
use pf_core::fixture::{
    FixtureCacheCapture, FixtureEffectsCapture, FixtureModelCapture, FixtureSpec,
    FixtureTraceCapture, FixtureWorldCapture,
};
use pf_core::manifest::AgentInfo;
use pf_core::snapshot::Snapshotter;
use pf_core::store::PfStore;
use pf_registry::{ImageRef, OciRegistry, Registry};
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn snapshotter() -> Snapshotter {
    let agent = AgentInfo {
        kind: "test".into(),
        version: "0".into(),
        fingerprint: "oci-test".into(),
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

/// In-process fake OCI registry. Tracks uploaded blobs by digest and
/// the manifest pushed to the test repo's tag.
#[derive(Clone, Default)]
struct FakeOci {
    /// digest "sha256:<hex>" → bytes
    blobs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// path "/v2/<repo>/manifests/<tag>" → manifest body
    manifests: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// upload session id → bytes accumulated so far (we only support
    /// monolithic single-PUT uploads, so this stays empty).
    uploads: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

struct StartUploadResponder(FakeOci);
impl Respond for StartUploadResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let id = format!("upload-{}", uuid_like());
        self.0
            .uploads
            .lock()
            .unwrap()
            .insert(id.clone(), Vec::new());
        // Return the upload URL (relative is fine — the client prefixes
        // with the registry base).
        let location = req
            .url
            .path()
            .replace("/blobs/uploads/", &format!("/blobs/uploads/{id}"));
        ResponseTemplate::new(202).insert_header("Location", &location)
    }
}

struct PutBlobResponder(FakeOci);
impl Respond for PutBlobResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        // ?digest=sha256:<hex>
        let digest = req
            .url
            .query_pairs()
            .find(|(k, _)| k == "digest")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        if digest.is_empty() {
            return ResponseTemplate::new(400);
        }
        self.0
            .blobs
            .lock()
            .unwrap()
            .insert(digest, req.body.clone());
        ResponseTemplate::new(201)
    }
}

struct HeadBlobResponder(FakeOci);
impl Respond for HeadBlobResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let digest = req.url.path().rsplit('/').next().unwrap_or("").to_owned();
        if self.0.blobs.lock().unwrap().contains_key(&digest) {
            ResponseTemplate::new(200)
        } else {
            ResponseTemplate::new(404)
        }
    }
}

struct GetBlobResponder(FakeOci);
impl Respond for GetBlobResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let digest = req.url.path().rsplit('/').next().unwrap_or("").to_owned();
        match self.0.blobs.lock().unwrap().get(&digest) {
            Some(b) => ResponseTemplate::new(200).set_body_bytes(b.clone()),
            None => ResponseTemplate::new(404),
        }
    }
}

struct PutManifestResponder(FakeOci);
impl Respond for PutManifestResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        self.0
            .manifests
            .lock()
            .unwrap()
            .insert(req.url.path().to_owned(), req.body.clone());
        ResponseTemplate::new(201)
    }
}

struct GetManifestResponder(FakeOci);
impl Respond for GetManifestResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        match self.0.manifests.lock().unwrap().get(req.url.path()) {
            Some(b) => ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/vnd.oci.image.manifest.v1+json")
                .set_body_bytes(b.clone()),
            None => ResponseTemplate::new(404),
        }
    }
}

struct HeadManifestResponder(FakeOci);
impl Respond for HeadManifestResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        if self
            .0
            .manifests
            .lock()
            .unwrap()
            .contains_key(req.url.path())
        {
            ResponseTemplate::new(200)
        } else {
            ResponseTemplate::new(404)
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
async fn oci_full_round_trip_via_wiremock() {
    let server = MockServer::start().await;
    let state = FakeOci::default();

    // POST /v2/<repo>/blobs/uploads/ → 202 + Location
    Mock::given(method("POST"))
        .and(path_regex(r"^/v2/.+/blobs/uploads/$"))
        .respond_with(StartUploadResponder(state.clone()))
        .mount(&server)
        .await;

    // PUT /v2/<repo>/blobs/uploads/<id>?digest=...
    Mock::given(method("PUT"))
        .and(path_regex(r"^/v2/.+/blobs/uploads/.+$"))
        .respond_with(PutBlobResponder(state.clone()))
        .mount(&server)
        .await;

    // HEAD /v2/<repo>/blobs/sha256:<hex>
    Mock::given(method("HEAD"))
        .and(path_regex(r"^/v2/.+/blobs/sha256:.+$"))
        .respond_with(HeadBlobResponder(state.clone()))
        .mount(&server)
        .await;

    // GET /v2/<repo>/blobs/sha256:<hex>
    Mock::given(method("GET"))
        .and(path_regex(r"^/v2/.+/blobs/sha256:.+$"))
        .respond_with(GetBlobResponder(state.clone()))
        .mount(&server)
        .await;

    // PUT /v2/<repo>/manifests/<tag>
    Mock::given(method("PUT"))
        .and(path_regex(r"^/v2/.+/manifests/.+$"))
        .respond_with(PutManifestResponder(state.clone()))
        .mount(&server)
        .await;

    // GET /v2/<repo>/manifests/<tag>
    Mock::given(method("GET"))
        .and(path_regex(r"^/v2/.+/manifests/.+$"))
        .respond_with(GetManifestResponder(state.clone()))
        .mount(&server)
        .await;

    // HEAD /v2/<repo>/manifests/<tag>  (used by exists())
    Mock::given(method("HEAD"))
        .and(path_regex(r"^/v2/.+/manifests/.+$"))
        .respond_with(HeadManifestResponder(state.clone()))
        .mount(&server)
        .await;

    // Snapshot.
    let local_dir = TempDir::new().unwrap();
    let local_store = PfStore::open(local_dir.path()).unwrap();
    let cid = snapshotter().snapshot(&local_store, vec![]).unwrap();
    let manifest = local_store.get_manifest(&cid).unwrap();

    // Push via OCI to the wiremock server.
    let host_port = server.address();
    let target = ImageRef::Oci {
        host: host_port.ip().to_string(),
        port: Some(host_port.port()),
        repo: "alice/sess".into(),
        tag: Some("v1".into()),
    };
    let reg = OciRegistry::new(std::collections::BTreeMap::new()).with_sign_key("test-key");

    reg.push(&target, &manifest, local_store.blobs())
        .await
        .expect("OCI push");

    // exists() lights up after push.
    assert!(reg.exists(&target).await.unwrap(), "exists after push");

    // Pull and confirm round-trip.
    let pulled = reg.pull(&target).await.expect("OCI pull");
    assert_eq!(pulled.manifest.schema_version, 1);

    let mem = MemBlobStore::new();
    for (digest, bytes) in &pulled.blobs {
        let put_d = mem.put(bytes).unwrap();
        assert_eq!(&put_d, digest, "pulled blob hashes to its digest");
    }
    let mem_cid = mem
        .put(&serde_json::to_vec(&pulled.manifest).unwrap())
        .unwrap();
    assert_eq!(
        mem_cid, cid,
        "manifest CID round-trips through OCI push+pull"
    );
}

#[tokio::test]
async fn oci_pull_missing_manifest_returns_backend_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let host_port = server.address();
    let target = ImageRef::Oci {
        host: host_port.ip().to_string(),
        port: Some(host_port.port()),
        repo: "absent/repo".into(),
        tag: None,
    };
    let reg = OciRegistry::new(std::collections::BTreeMap::new());
    let r = reg.pull(&target).await.unwrap_err();
    let msg = format!("{r}");
    assert!(msg.contains("404"), "expected 404 in error, got: {msg}");
}
