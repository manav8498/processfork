// SPDX-License-Identifier: MIT
//! End-to-end S3 registry round-trip against a wiremock server that
//! speaks just enough of the v4 PUT/GET/HEAD/LIST protocol.
//!
//! Confirms the v1.0.2 s3:// adapter actually:
//!   1. PUTs `<prefix>/manifest.json` + `manifest.json.sig`,
//!   2. PUTs every transitively-reachable blob under blobs/sha256/...,
//!   3. on pull: GETs the manifest, verifies signature, lists the
//!      blob keys via `?list-type=2&prefix=...`, GETs and verifies
//!      every blob.
//!
//! Uses path-style addressing (`force_path_style: true`) so all
//! requests target `<endpoint>/<bucket>/<key>` — much easier to mock
//! than virtual-host style.

#![cfg(feature = "s3-live")]

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use pf_core::cas::{BlobStore, MemBlobStore};
use pf_core::fixture::{
    FixtureCacheCapture, FixtureEffectsCapture, FixtureModelCapture, FixtureSpec,
    FixtureTraceCapture, FixtureWorldCapture,
};
use pf_core::manifest::AgentInfo;
use pf_core::snapshot::Snapshotter;
use pf_core::store::PfStore;
use pf_registry::{ImageRef, Registry, S3Registry};
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn snapshotter() -> Snapshotter {
    let agent = AgentInfo {
        kind: "test".into(),
        version: "0".into(),
        fingerprint: "s3-test".into(),
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
struct FakeBucket {
    objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

struct PutResponder(FakeBucket);
impl Respond for PutResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        // path is `/<bucket>/<key>`; key may itself contain `/`.
        let key = strip_bucket(req.url.path());
        self.0.objects.lock().unwrap().insert(key, req.body.clone());
        ResponseTemplate::new(200)
    }
}

struct GetResponder(FakeBucket);
impl Respond for GetResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let key = strip_bucket(req.url.path());
        match self.0.objects.lock().unwrap().get(&key) {
            Some(b) => ResponseTemplate::new(200).set_body_bytes(b.clone()),
            None => ResponseTemplate::new(404),
        }
    }
}

struct HeadResponder(FakeBucket);
impl Respond for HeadResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let key = strip_bucket(req.url.path());
        if self.0.objects.lock().unwrap().contains_key(&key) {
            ResponseTemplate::new(200)
        } else {
            ResponseTemplate::new(404)
        }
    }
}

/// `GET /<bucket>?list-type=2&prefix=...` → minimal XML response with
/// every key whose path starts with `prefix`. The aws-sdk-s3 client
/// parses this into a `ListObjectsV2Output`.
struct ListResponder(FakeBucket);
impl Respond for ListResponder {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let prefix = req
            .url
            .query_pairs()
            .find(|(k, _)| k == "prefix")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        let bucket_name = req.url.path().trim_start_matches('/').trim_end_matches('/');
        let matches: Vec<String> = self
            .0
            .objects
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        let mut body = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">"#,
        );
        let _ = write!(body, "<Name>{bucket_name}</Name>");
        let _ = write!(body, "<Prefix>{prefix}</Prefix>");
        let _ = write!(body, "<KeyCount>{}</KeyCount>", matches.len());
        body.push_str("<MaxKeys>1000</MaxKeys>");
        body.push_str("<IsTruncated>false</IsTruncated>");
        for k in matches {
            let _ = write!(
                body,
                "<Contents><Key>{}</Key><Size>0</Size><ETag>\"x\"</ETag></Contents>",
                xml_escape(&k)
            );
        }
        body.push_str("</ListBucketResult>");
        ResponseTemplate::new(200)
            .insert_header("Content-Type", "application/xml")
            .set_body_string(body)
    }
}

fn strip_bucket(path: &str) -> String {
    // path-style: "/<bucket>/<key...>". Drop the leading slash and the
    // first segment.
    let trimmed = path.trim_start_matches('/');
    let mut split = trimmed.splitn(2, '/');
    let _bucket = split.next().unwrap_or("");
    split.next().unwrap_or("").to_owned()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[tokio::test]
async fn s3_full_round_trip_via_wiremock() {
    let server = MockServer::start().await;
    let state = FakeBucket::default();

    // GET /<bucket>?list-type=2&prefix=... — must be matched BEFORE
    // the bare GET /<bucket>/<key> because both match the same path
    // shape; wiremock picks the most-recently-mounted match for a
    // tie. Mount LIST first so the tie goes the other way (later
    // mounts win, so plain GET registered last).
    Mock::given(method("GET"))
        .and(path_regex(r"^/test-bucket/?$"))
        .respond_with(ListResponder(state.clone()))
        .mount(&server)
        .await;

    // PUT /<bucket>/<key>
    Mock::given(method("PUT"))
        .and(path_regex(r"^/test-bucket/.+$"))
        .respond_with(PutResponder(state.clone()))
        .mount(&server)
        .await;

    // HEAD /<bucket>/<key>
    Mock::given(method("HEAD"))
        .and(path_regex(r"^/test-bucket/.+$"))
        .respond_with(HeadResponder(state.clone()))
        .mount(&server)
        .await;

    // GET /<bucket>/<key>
    Mock::given(method("GET"))
        .and(path_regex(r"^/test-bucket/.+$"))
        .respond_with(GetResponder(state.clone()))
        .mount(&server)
        .await;

    // Snapshot.
    let local_dir = TempDir::new().unwrap();
    let local_store = PfStore::open(local_dir.path()).unwrap();
    let cid = snapshotter().snapshot(&local_store, vec![]).unwrap();
    let manifest = local_store.get_manifest(&cid).unwrap();

    // Build registry pointed at the wiremock server.
    let mut auth = BTreeMap::new();
    auth.insert("AWS_ACCESS_KEY_ID".into(), "test-key".into());
    auth.insert("AWS_SECRET_ACCESS_KEY".into(), "test-secret".into());
    auth.insert("AWS_REGION".into(), "us-east-1".into());
    auth.insert("AWS_ENDPOINT_URL".into(), server.uri());
    let reg = S3Registry::new(auth).with_sign_key("s3-test-key");

    let target = ImageRef::S3 {
        bucket: "test-bucket".into(),
        prefix: "alice/sess".into(),
    };

    reg.push(&target, &manifest, local_store.blobs())
        .await
        .expect("S3 push");

    assert!(reg.exists(&target).await.unwrap(), "exists after push");

    let pulled = reg.pull(&target).await.expect("S3 pull");
    assert_eq!(pulled.manifest.schema_version, 1);

    let mem = MemBlobStore::new();
    for (digest, bytes) in &pulled.blobs {
        let put_d = mem.put(bytes).unwrap();
        assert_eq!(&put_d, digest, "pulled blob hashes to its digest");
    }
    let mem_cid = mem
        .put(&serde_json::to_vec(&pulled.manifest).unwrap())
        .unwrap();
    assert_eq!(mem_cid, cid, "manifest CID round-trips through S3");
}

#[tokio::test]
async fn s3_pull_missing_manifest_returns_backend_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let mut auth = BTreeMap::new();
    auth.insert("AWS_ACCESS_KEY_ID".into(), "x".into());
    auth.insert("AWS_SECRET_ACCESS_KEY".into(), "y".into());
    auth.insert("AWS_ENDPOINT_URL".into(), server.uri());
    let reg = S3Registry::new(auth);

    let target = ImageRef::S3 {
        bucket: "absent".into(),
        prefix: "nope".into(),
    };
    let r = reg.pull(&target).await.unwrap_err();
    let msg = format!("{r}");
    assert!(
        msg.contains("404") || msg.contains("S3 GetObject"),
        "unexpected error: {msg}"
    );
}
