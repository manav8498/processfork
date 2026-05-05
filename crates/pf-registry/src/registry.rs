// SPDX-License-Identifier: MIT
//! The shared `Registry` trait. Adapter authors implement this; the CLI
//! and SDKs talk to it.

use async_trait::async_trait;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::Manifest;

use crate::image_ref::ImageRef;

/// Backend errors. Adapter-specific failures (HTTP, S3 SigV4, IPFS daemon
/// down, etc.) flow through `Backend`. Trait-level invariants surface as
/// the typed variants above it.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// Pull / push attempted against a scheme this build doesn't support
    /// (feature flag off).
    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),
    /// Reference parser caught a malformed URL.
    #[error("image ref: {0}")]
    BadRef(#[from] crate::image_ref::ImageRefError),
    /// Wrapped `pf-core` failure (CAS, manifest decode, …).
    #[error("core: {0}")]
    Core(#[from] pf_core::Error),
    /// Adapter-specific backend failure (HTTP, S3, IPFS, …).
    #[error("backend: {0}")]
    Backend(String),
    /// Manifest signature did not verify.
    #[error("signature verify failed: {0}")]
    SignatureVerify(String),
}

/// Output of [`Registry::pull`]: the manifest plus every blob it references.
#[derive(Debug)]
pub struct LayerSet {
    pub manifest: Manifest,
    pub blobs: Vec<(Digest256, Vec<u8>)>,
}

/// A registry. Push uploads a manifest + every reachable blob; pull
/// returns both. Implementations MUST be `Send + Sync`.
#[async_trait]
pub trait Registry: Send + Sync {
    /// Push a `.pfimg` to the destination encoded by `target`.
    async fn push(
        &self,
        target: &ImageRef,
        manifest: &Manifest,
        blobs: &dyn BlobStore,
    ) -> Result<(), RegistryError>;

    /// Pull a `.pfimg` from `source`. Returns the manifest + all blobs
    /// it references (so the caller can flush them into a local
    /// [`BlobStore`] of choice).
    async fn pull(&self, source: &ImageRef) -> Result<LayerSet, RegistryError>;

    /// Cheap existence check (HEAD-style; doesn't pull blobs).
    async fn exists(&self, source: &ImageRef) -> Result<bool, RegistryError>;
}

/// Walk the manifest's layer descriptors and yield every TOP-LEVEL
/// digest. Use [`transitive_blob_digests`] when you actually want to
/// materialise the full set (file blobs inside the FsTree, page blobs
/// inside the PageManifest, etc.).
#[must_use]
pub fn manifest_blob_digests(m: &Manifest) -> Vec<Digest256> {
    vec![
        m.model.base.clone(),
        m.model.diff.clone(),
        m.cache.manifest.clone(),
        m.world.fs.clone(),
        m.world.env.clone(),
        m.world.procs.clone(),
        m.effects.ledger.clone(),
        m.trace.messages.clone(),
    ]
}

/// Walk every blob the manifest transitively references — including
/// file blobs inside the world-layer `FsTree` and K/V page blobs inside
/// the cache-layer `PageManifest`.
///
/// Returns digests in deterministic insertion order with duplicates
/// removed. The model / effects / trace layers are self-contained in
/// their top-level digest (JSON envelope or JSONL with no nested blob
/// refs), so they aren't expanded here.
pub fn transitive_blob_digests(
    m: &Manifest,
    blobs: &dyn BlobStore,
) -> Result<Vec<Digest256>, RegistryError> {
    use std::collections::BTreeSet;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut order: Vec<Digest256> = Vec::new();

    // Top-level descriptors first.
    for d in manifest_blob_digests(m) {
        if seen.insert(d.as_str().to_owned()) {
            order.push(d);
        }
    }

    // World FsTree → file / symlink blobs.
    if let Ok(fs_bytes) = blobs.get(&m.world.fs) {
        if let Ok(tree) = serde_json::from_slice::<serde_json::Value>(&fs_bytes) {
            if tree.get("kind").and_then(|v| v.as_str()) == Some("fs.tree.v1") {
                if let Some(entries) = tree.get("entries").and_then(|v| v.as_array()) {
                    for e in entries {
                        if let Some(blob) = e.get("blob").and_then(|v| v.as_str()) {
                            if let Ok(d) = Digest256::parse(blob) {
                                if seen.insert(d.as_str().to_owned()) {
                                    order.push(d);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Cache PageManifest → per-page K and V blobs.
    if let Ok(pm_bytes) = blobs.get(&m.cache.manifest) {
        if let Ok(pm) = serde_json::from_slice::<serde_json::Value>(&pm_bytes) {
            if let Some(pages) = pm.get("pages").and_then(|v| v.as_array()) {
                for p in pages {
                    for key in ["k", "v"] {
                        if let Some(s) = p.get(key).and_then(|v| v.as_str()) {
                            if let Ok(d) = Digest256::parse(s) {
                                if seen.insert(d.as_str().to_owned()) {
                                    order.push(d);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(order)
}
