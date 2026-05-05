// SPDX-License-Identifier: MIT
//! High-level store: a [`BlobStore`] plus a manifest catalog.
//!
//! `PfStore` is the single object the CLI / SDKs talk to for read/write of
//! whole `.pfimg` images. It hides the distinction between blobs (CAS) and
//! manifests (also CAS, but with a separate index for `pf log`).

use crate::cas::{BlobStore, FsBlobStore};
use crate::digest::Digest256;
use crate::error::Result;
use crate::manifest::Manifest;

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A `PfStore` is the local-filesystem entry point. It owns one [`BlobStore`]
/// (default [`FsBlobStore`]) plus an `images/` directory of one symlink per
/// manifest digest, kept for fast `pf log` traversal.
pub struct PfStore {
    blobs: Arc<dyn BlobStore>,
    root: PathBuf,
}

impl PfStore {
    /// Open the default on-disk store rooted at `root`.
    ///
    /// Creates `root/blobs/sha256/` and `root/images/` if missing.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(root.join("images"))?;
        let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(&root)?);
        Ok(Self { blobs, root })
    }

    /// Open with a caller-supplied blob store (useful for in-memory tests).
    pub fn with_blobstore(root: impl AsRef<Path>, blobs: Arc<dyn BlobStore>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(root.join("images"))?;
        Ok(Self { blobs, root })
    }

    /// Borrow the underlying blob store.
    pub fn blobs(&self) -> &dyn BlobStore {
        self.blobs.as_ref()
    }

    /// Borrow the underlying blob store as an `Arc` (for cloning into capture
    /// pipelines).
    pub fn blobs_arc(&self) -> Arc<dyn BlobStore> {
        Arc::clone(&self.blobs)
    }

    /// Root directory of the store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Persist a [`Manifest`] and return its content-id (the digest of its
    /// canonical JSON serialization).
    ///
    /// Also drops a marker file at `images/<cid>.json` so `pf log` can walk
    /// without scanning every blob.
    pub fn put_manifest(&self, m: &Manifest) -> Result<Digest256> {
        let json = serde_json::to_vec(m)?;
        let cid = self.blobs.put(&json)?;
        let marker = self.root.join("images").join(format!("{}.json", cid.hex()));
        if !marker.exists() {
            std::fs::write(&marker, &json)?;
        }
        Ok(cid)
    }

    /// Load a manifest by content-id.
    pub fn get_manifest(&self, cid: &Digest256) -> Result<Manifest> {
        let bytes = self.blobs.get(cid)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Iterate every manifest known to this store. Order is unspecified.
    pub fn iter_manifests(&self) -> Result<impl Iterator<Item = (Digest256, Manifest)> + '_> {
        let entries = std::fs::read_dir(self.root.join("images"))?;
        Ok(entries.filter_map(move |e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            let hex = name.strip_suffix(".json")?;
            let cid = Digest256::parse(&format!("sha256:{hex}")).ok()?;
            let m: Manifest = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            Some((cid, m))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{
        AgentInfo, CacheLayer, EffectsLayer, MEDIATYPE_V1, ModelLayer, TraceLayer, WorldLayer,
    };
    use chrono::Utc;
    use tempfile::TempDir;

    fn fixture(blobs: &dyn BlobStore) -> Manifest {
        let d = blobs.put(b"x").unwrap();
        Manifest {
            schema_version: 1,
            media_type: MEDIATYPE_V1.to_owned(),
            agent: AgentInfo {
                kind: "test".into(),
                version: "0".into(),
                fingerprint: "f".into(),
            },
            model: ModelLayer {
                base: d.clone(),
                diff: d.clone(),
            },
            cache: CacheLayer {
                layout: "paged-batchinvariant-v1".into(),
                manifest: d.clone(),
            },
            world: WorldLayer {
                fs: d.clone(),
                env: d.clone(),
                procs: d.clone(),
            },
            effects: EffectsLayer { ledger: d.clone() },
            trace: TraceLayer { messages: d },
            created_at: Utc::now(),
            parents: vec![],
        }
    }

    #[test]
    fn put_get_manifest_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let m = fixture(store.blobs());
        let cid = store.put_manifest(&m).unwrap();
        let back = store.get_manifest(&cid).unwrap();
        assert_eq!(back.schema_version, 1);
        assert_eq!(back.parents.len(), 0);
    }

    #[test]
    fn iter_manifests_lists_what_was_written() {
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let m1 = fixture(store.blobs());
        let cid1 = store.put_manifest(&m1).unwrap();
        let mut m2 = m1.clone();
        m2.agent.version = "1".into();
        let cid2 = store.put_manifest(&m2).unwrap();
        assert_ne!(cid1, cid2);
        let listed: std::collections::HashSet<_> = store
            .iter_manifests()
            .unwrap()
            .map(|(digest, _)| digest)
            .collect();
        assert!(listed.contains(&cid1));
        assert!(listed.contains(&cid2));
    }
}
