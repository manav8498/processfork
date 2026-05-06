// SPDX-License-Identifier: MIT
//! Phase-9 acceptance: a full `.pfimg` round-trips through the
//! filesystem-backed registry end-to-end (manifest + every layer blob),
//! the cosign-shaped signature verifies, and tampering is caught.
//!
//! HF / S3 / IPFS adapters check that they cleanly return
//! `UnsupportedScheme` until v1.0.1 wires them up.

use std::sync::Arc;

use pf_core::cas::{BlobStore, MemBlobStore};
use pf_core::fixture::{
    FixtureCacheCapture, FixtureEffectsCapture, FixtureModelCapture, FixtureSpec,
    FixtureTraceCapture, FixtureWorldCapture,
};
use pf_core::manifest::AgentInfo;
use pf_core::snapshot::Snapshotter;
use pf_core::store::PfStore;
use pf_registry::{
    FileRegistry, HfRegistry, ImageRef, IpfsRegistry, Registry, RegistryError, S3Registry,
};
use tempfile::TempDir;

fn snapshotter() -> Snapshotter {
    let agent = AgentInfo {
        kind: "test".into(),
        version: "0".into(),
        fingerprint: "registry-test".into(),
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

#[tokio::test]
async fn file_registry_full_round_trip() {
    let local_dir = TempDir::new().unwrap();
    let local_store = PfStore::open(local_dir.path()).unwrap();
    let cid = snapshotter().snapshot(&local_store, vec![]).unwrap();
    let manifest = local_store.get_manifest(&cid).unwrap();

    // Push to a file:// registry.
    let registry_dir = TempDir::new().unwrap();
    let target = ImageRef::File {
        path: registry_dir.path().to_path_buf(),
    };
    let reg = FileRegistry::new();
    reg.push(&target, &manifest, local_store.blobs())
        .await
        .unwrap();

    // exists() should be true now.
    assert!(reg.exists(&target).await.unwrap());

    // Pull into a fresh in-memory blob store.
    let pulled = reg.pull(&target).await.unwrap();
    assert_eq!(pulled.manifest.schema_version, 1);
    let mem = MemBlobStore::new();
    for (digest, bytes) in &pulled.blobs {
        let put_d = mem.put(bytes).unwrap();
        assert_eq!(&put_d, digest, "blob digest re-derived from bytes");
    }

    // The merged manifest reachable from the new mem store?
    let mem_cid = mem
        .put(&serde_json::to_vec(&pulled.manifest).unwrap())
        .unwrap();
    assert_eq!(mem_cid, cid, "round-trip manifest CID is identical");
}

#[tokio::test]
async fn pull_detects_tampered_manifest() {
    let local_dir = TempDir::new().unwrap();
    let local_store = PfStore::open(local_dir.path()).unwrap();
    let cid = snapshotter().snapshot(&local_store, vec![]).unwrap();
    let manifest = local_store.get_manifest(&cid).unwrap();

    let registry_dir = TempDir::new().unwrap();
    let target = ImageRef::File {
        path: registry_dir.path().to_path_buf(),
    };
    let reg = FileRegistry::new();
    reg.push(&target, &manifest, local_store.blobs())
        .await
        .unwrap();

    // Corrupt the manifest in the registry.
    let manifest_path = registry_dir.path().join("manifest.json");
    let mut bytes = std::fs::read(&manifest_path).unwrap();
    bytes.extend_from_slice(b" tampered ");
    std::fs::write(&manifest_path, bytes).unwrap();

    let err = reg.pull(&target).await.unwrap_err();
    assert!(matches!(err, RegistryError::SignatureVerify(_)));
}

#[tokio::test]
async fn pull_detects_tampered_blob() {
    let local_dir = TempDir::new().unwrap();
    let local_store = PfStore::open(local_dir.path()).unwrap();
    let cid = snapshotter().snapshot(&local_store, vec![]).unwrap();
    let manifest = local_store.get_manifest(&cid).unwrap();

    let registry_dir = TempDir::new().unwrap();
    let target = ImageRef::File {
        path: registry_dir.path().to_path_buf(),
    };
    let reg = FileRegistry::new();
    reg.push(&target, &manifest, local_store.blobs())
        .await
        .unwrap();

    // Corrupt one of the blob shards.
    let blobs_root = registry_dir.path().join("blobs").join("sha256");
    let mut shards = std::fs::read_dir(&blobs_root).unwrap();
    let shard = shards.next().unwrap().unwrap();
    let blob = std::fs::read_dir(shard.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    std::fs::write(blob.path(), b"\x28\xb5\x2f\xfd\x00garbage").unwrap();

    let err = reg.pull(&target).await.unwrap_err();
    assert!(matches!(
        err,
        RegistryError::Backend(_) | RegistryError::Core(_)
    ));
}

#[tokio::test]
async fn cow_dedupes_pushed_blobs() {
    let local_dir = TempDir::new().unwrap();
    let local_store = PfStore::open(local_dir.path()).unwrap();

    // Push the same fixture twice into the same registry dir; the second
    // push must be cheap (every blob already on disk).
    let cid = snapshotter().snapshot(&local_store, vec![]).unwrap();
    let manifest = local_store.get_manifest(&cid).unwrap();

    let registry_dir = TempDir::new().unwrap();
    let target = ImageRef::File {
        path: registry_dir.path().to_path_buf(),
    };
    let reg = FileRegistry::new();
    reg.push(&target, &manifest, local_store.blobs())
        .await
        .unwrap();
    let after_first = walk_size(registry_dir.path());
    reg.push(&target, &manifest, local_store.blobs())
        .await
        .unwrap();
    let after_second = walk_size(registry_dir.path());
    assert_eq!(
        after_first, after_second,
        "second push of identical content must not grow the registry"
    );
}

fn walk_size(dir: &std::path::Path) -> u64 {
    let mut total = 0;
    for entry in walkdir::WalkDir::new(dir) {
        let Ok(e) = entry else { continue };
        if e.file_type().is_file() {
            total += e.metadata().map_or(0, |m| m.len());
        }
    }
    total
}

/// HF adapter shape sanity-check. Without an HF token the live adapter
/// can still call into resolve()/exists() against an unreachable
/// endpoint and return a Backend error (not a panic). Live round-trip
/// coverage lives in `hf_round_trip.rs` against a wiremock server.
#[cfg(feature = "hf-live")]
#[tokio::test]
async fn hf_adapter_no_token_pull_errors_cleanly() {
    let reg = HfRegistry::new(None).with_endpoint("http://127.0.0.1:1");
    let r = reg
        .pull(&ImageRef::Hf {
            user: "x".into(),
            repo: "y".into(),
            tag: None,
        })
        .await
        .unwrap_err();
    // No panic, no UnsupportedScheme — clean Backend error from the
    // unreachable endpoint.
    assert!(matches!(r, RegistryError::Backend(_)));
}

/// In a build without `hf-live`, the adapter still returns
/// UnsupportedScheme so callers get a clear migration message.
#[cfg(not(feature = "hf-live"))]
#[tokio::test]
async fn hf_adapter_returns_unsupported_when_feature_off() {
    let reg = HfRegistry::new(None);
    let r = reg
        .pull(&ImageRef::Hf {
            user: "x".into(),
            repo: "y".into(),
            tag: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(r, RegistryError::UnsupportedScheme(_)));
}

/// Live S3 build (default since v1.0.2). Without an endpoint or
/// creds the SDK returns a Backend error trying to talk to AWS;
/// real round-trip coverage lives in `s3_round_trip.rs`.
#[cfg(feature = "s3-live")]
#[tokio::test]
async fn s3_adapter_no_endpoint_pull_errors_cleanly() {
    let mut auth = std::collections::BTreeMap::new();
    // Point at an unreachable endpoint so we don't accidentally hit
    // real AWS; either way the result is Backend(...), not panic /
    // UnsupportedScheme.
    auth.insert("AWS_ENDPOINT_URL".into(), "http://127.0.0.1:1".into());
    auth.insert("AWS_ACCESS_KEY_ID".into(), "x".into());
    auth.insert("AWS_SECRET_ACCESS_KEY".into(), "y".into());
    let reg = S3Registry::new(auth);
    let r = reg
        .pull(&ImageRef::S3 {
            bucket: "b".into(),
            prefix: "p".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(r, RegistryError::Backend(_)));
}

/// In a build without `s3-live`, the adapter still returns
/// UnsupportedScheme so callers get a clear migration message.
#[cfg(not(feature = "s3-live"))]
#[tokio::test]
async fn s3_adapter_returns_unsupported_when_feature_off() {
    let reg = S3Registry::new(std::collections::BTreeMap::default());
    let r = reg
        .pull(&ImageRef::S3 {
            bucket: "b".into(),
            prefix: "p".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(r, RegistryError::UnsupportedScheme(_)));
}

#[tokio::test]
async fn ipfs_adapter_returns_unsupported_in_default_build() {
    let reg = IpfsRegistry::new("http://127.0.0.1:5001".into());
    let r = reg
        .pull(&ImageRef::Ipfs { cid: "bafy".into() })
        .await
        .unwrap_err();
    assert!(matches!(r, RegistryError::UnsupportedScheme(_)));
}

#[tokio::test]
async fn open_dispatches_to_correct_adapter() {
    use std::collections::BTreeMap;
    let auth = BTreeMap::new();
    let r = pf_registry::open(
        &ImageRef::File {
            path: std::path::PathBuf::from("/tmp/x"),
        },
        &auth,
    )
    .unwrap();
    assert!(
        !r.exists(&ImageRef::File {
            path: std::path::PathBuf::from("/tmp/does-not-exist"),
        })
        .await
        .unwrap()
    );
}
