// SPDX-License-Identifier: MIT
//! Filesystem-backed registry — the build-host test backbone, also useful
//! for air-gapped offline transport (`pf push file:///mnt/usb/...`).
//!
//! Layout (rooted at `<file:// path>/<repo>/`):
//!
//! ```text
//! <root>/
//!   manifest.json
//!   manifest.json.sig            (Phase-9 self-signed; verify on pull)
//!   blobs/sha256/<aa>/<aabbccdd…>.zst
//! ```

use async_trait::async_trait;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::Manifest;

use crate::image_ref::ImageRef;
use crate::registry::{LayerSet, Registry, RegistryError, transitive_blob_digests};
use crate::sign::{sign_manifest, verify_manifest};

/// Construct with [`FileRegistry::new`]. Stateless except for the cosign
/// stub; configurable via env vars (`PF_FILE_REG_SIGN_KEY`).
#[derive(Default, Debug)]
pub struct FileRegistry {
    /// Optional signing-key string; if `None` we use the v1 stub
    /// HMAC-style self-signature with a fixed key. Real cosign keys
    /// land in v1.1 behind the `sigstore-keyless` feature.
    sign_key: Option<String>,
}

impl FileRegistry {
    pub fn new() -> Self {
        Self {
            sign_key: std::env::var("PF_FILE_REG_SIGN_KEY").ok(),
        }
    }

    #[must_use]
    pub fn with_sign_key(mut self, key: impl Into<String>) -> Self {
        self.sign_key = Some(key.into());
        self
    }

    fn root(target: &ImageRef) -> Result<&std::path::Path, RegistryError> {
        match target {
            ImageRef::File { path } => Ok(path.as_path()),
            other => Err(RegistryError::Backend(format!(
                "FileRegistry called with non-file ref {other:?}"
            ))),
        }
    }
}

#[async_trait]
impl Registry for FileRegistry {
    async fn push(
        &self,
        target: &ImageRef,
        manifest: &Manifest,
        blobs: &dyn BlobStore,
    ) -> Result<(), RegistryError> {
        let root = Self::root(target)?;
        std::fs::create_dir_all(root.join("blobs").join("sha256"))
            .map_err(|e| RegistryError::Backend(format!("mkdir: {e}")))?;

        // 1. write manifest.json (canonical form: serde_json::to_vec).
        let manifest_bytes = serde_json::to_vec(manifest)
            .map_err(|e| RegistryError::Backend(format!("manifest serialize: {e}")))?;
        std::fs::write(root.join("manifest.json"), &manifest_bytes)
            .map_err(|e| RegistryError::Backend(format!("write manifest: {e}")))?;

        // 2. cosign-shaped signature next to the manifest.
        let sig = sign_manifest(&manifest_bytes, self.sign_key.as_deref());
        std::fs::write(
            root.join("manifest.json.sig"),
            serde_json::to_vec(&sig).unwrap(),
        )
        .map_err(|e| RegistryError::Backend(format!("write sig: {e}")))?;

        // 3. copy every transitively-reachable blob (top-level layer
        //    descriptors PLUS the file blobs inside the FsTree and the
        //    K/V page blobs inside the PageManifest).
        for digest in transitive_blob_digests(manifest, blobs)? {
            copy_blob(blobs, root, &digest)?;
        }
        Ok(())
    }

    async fn pull(&self, source: &ImageRef) -> Result<LayerSet, RegistryError> {
        let root = Self::root(source)?;
        let manifest_bytes = std::fs::read(root.join("manifest.json"))
            .map_err(|e| RegistryError::Backend(format!("read manifest: {e}")))?;

        // Verify signature BEFORE parsing the manifest.
        let sig_bytes = std::fs::read(root.join("manifest.json.sig"))
            .map_err(|e| RegistryError::Backend(format!("read sig: {e}")))?;
        let sig = serde_json::from_slice(&sig_bytes)
            .map_err(|e| RegistryError::SignatureVerify(format!("parse sig: {e}")))?;
        verify_manifest(&manifest_bytes, &sig, self.sign_key.as_deref())
            .map_err(RegistryError::SignatureVerify)?;

        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| RegistryError::Backend(format!("parse manifest: {e}")))?;

        // Pull every blob the registry has shard-mirror'd. We can't call
        // `transitive_blob_digests(&manifest, blobs)` here because we're
        // pulling INTO the local store (no blob-store yet to query for
        // the FsTree). Instead, mirror the registry's `blobs/sha256/`
        // tree wholesale; it's content-addressed, so duplicates dedup
        // and we never end up with extra unreachable blobs from a clean
        // push.
        let mut blobs = Vec::new();
        let blobs_root = root.join("blobs").join("sha256");
        if blobs_root.exists() {
            for shard in std::fs::read_dir(&blobs_root)
                .map_err(|e| RegistryError::Backend(format!("read blobs/: {e}")))?
            {
                let shard = shard.map_err(|e| RegistryError::Backend(format!("shard: {e}")))?;
                if !shard
                    .file_type()
                    .map_err(|e| RegistryError::Backend(format!("shard ft: {e}")))?
                    .is_dir()
                {
                    continue;
                }
                for blob in std::fs::read_dir(shard.path())
                    .map_err(|e| RegistryError::Backend(format!("read shard: {e}")))?
                {
                    let blob = blob.map_err(|e| RegistryError::Backend(format!("blob: {e}")))?;
                    let name = blob.file_name().to_string_lossy().to_string();
                    let hex = name.strip_suffix(".zst").unwrap_or(&name);
                    let Ok(d) = Digest256::parse(&format!("sha256:{hex}")) else {
                        continue;
                    };
                    let bytes = read_blob(root, &d)?;
                    blobs.push((d, bytes));
                }
            }
        }
        Ok(LayerSet { manifest, blobs })
    }

    async fn exists(&self, source: &ImageRef) -> Result<bool, RegistryError> {
        Ok(Self::root(source)?.join("manifest.json").exists())
    }
}

fn blob_path(root: &std::path::Path, d: &Digest256) -> std::path::PathBuf {
    let hex = d.hex();
    root.join("blobs")
        .join("sha256")
        .join(&hex[..2])
        .join(format!("{hex}.zst"))
}

fn copy_blob(
    blobs: &dyn BlobStore,
    root: &std::path::Path,
    d: &Digest256,
) -> Result<(), RegistryError> {
    let dest = blob_path(root, d);
    if dest.exists() {
        return Ok(());
    }
    let bytes = blobs.get(d)?;
    let compressed = zstd::encode_all(bytes.as_slice(), 19)
        .map_err(|e| RegistryError::Backend(format!("zstd encode: {e}")))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| RegistryError::Backend(format!("mkdir blob shard: {e}")))?;
    }
    std::fs::write(&dest, compressed)
        .map_err(|e| RegistryError::Backend(format!("write blob: {e}")))?;
    Ok(())
}

fn read_blob(root: &std::path::Path, d: &Digest256) -> Result<Vec<u8>, RegistryError> {
    let src = blob_path(root, d);
    let compressed = std::fs::read(&src)
        .map_err(|e| RegistryError::Backend(format!("read blob {}: {e}", src.display())))?;
    let bytes = zstd::decode_all(compressed.as_slice())
        .map_err(|e| RegistryError::Backend(format!("zstd decode: {e}")))?;
    let observed = Digest256::of(&bytes);
    if &observed != d {
        return Err(RegistryError::Core(pf_core::Error::Integrity(format!(
            "registry blob {d} hashes to {observed}"
        ))));
    }
    Ok(bytes)
}
