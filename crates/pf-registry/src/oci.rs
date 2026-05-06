// SPDX-License-Identifier: MIT
//! Local OCI registry adapter.
//!
//! Maps `oci://<host>[:<port>]/<repo>[:<tag>]` to an
//! [OCI Distribution Spec v2](https://github.com/opencontainers/distribution-spec/)
//! registry endpoint. Tested against `registry:2`, ghcr.io, and an
//! in-process wiremock harness.
//!
//! ## Manifest shape
//!
//! ProcessFork's `.pfimg` becomes an OCI artifact: the meaningful
//! payload (the ProcessFork manifest JSON) is the `config` blob with
//! mediatype `application/vnd.processfork.image.v1+json`, and every
//! transitively-reachable blob (zstd-compressed) becomes one OCI
//! `layer` descriptor. Layer paths inside the .pfimg are preserved
//! via the `org.processfork.path` annotation so a clone can rebuild
//! the same on-disk shape as the FileRegistry.
//!
//! ```jsonc
//! {
//!   "schemaVersion": 2,
//!   "mediaType": "application/vnd.oci.image.manifest.v1+json",
//!   "config": {
//!     "mediaType": "application/vnd.processfork.image.v1+json",
//!     "size": 12345,
//!     "digest": "sha256:..."
//!   },
//!   "layers": [
//!     {
//!       "mediaType": "application/vnd.processfork.signature.v1+json",
//!       "size": 256, "digest": "sha256:...",
//!       "annotations": {"org.processfork.path": "manifest.json.sig"}
//!     },
//!     {
//!       "mediaType": "application/vnd.processfork.layer.v1+zstd",
//!       "size": 4096, "digest": "sha256:...",
//!       "annotations": {"org.processfork.path": "blobs/sha256/aa/aabb….zst"}
//!     }
//!   ]
//! }
//! ```

#[cfg(feature = "oci-live")]
mod live;
#[cfg(feature = "oci-live")]
pub use live::OciRegistry;

// Stub when the `oci-live` feature is off: keep the public surface so
// downstream code compiles, but every method returns UnsupportedScheme.
#[cfg(not(feature = "oci-live"))]
use crate::image_ref::ImageRef;
#[cfg(not(feature = "oci-live"))]
use crate::registry::{LayerSet, Registry, RegistryError};
#[cfg(not(feature = "oci-live"))]
use async_trait::async_trait;
#[cfg(not(feature = "oci-live"))]
use pf_core::cas::BlobStore;
#[cfg(not(feature = "oci-live"))]
use pf_core::manifest::Manifest;

#[cfg(not(feature = "oci-live"))]
#[derive(Debug, Default)]
pub struct OciRegistry {
    _auth: std::collections::BTreeMap<String, String>,
}

#[cfg(not(feature = "oci-live"))]
impl OciRegistry {
    /// Construct an OCI registry stub. Rebuild with `--features oci-live`
    /// (default since v1.0.2) to enable the live HTTP path.
    pub fn new(auth: std::collections::BTreeMap<String, String>) -> Self {
        Self { _auth: auth }
    }
}

#[cfg(not(feature = "oci-live"))]
fn err() -> RegistryError {
    RegistryError::UnsupportedScheme(
        "oci:// — pf-registry built without `oci-live`. Rebuild with \
         `--features oci-live` (default since v1.0.2) to enable OCI."
            .into(),
    )
}

#[cfg(not(feature = "oci-live"))]
#[async_trait]
impl Registry for OciRegistry {
    async fn push(
        &self,
        _: &ImageRef,
        _: &Manifest,
        _: &dyn BlobStore,
    ) -> Result<(), RegistryError> {
        Err(err())
    }
    async fn pull(&self, _: &ImageRef) -> Result<LayerSet, RegistryError> {
        Err(err())
    }
    async fn exists(&self, _: &ImageRef) -> Result<bool, RegistryError> {
        Err(err())
    }
}
