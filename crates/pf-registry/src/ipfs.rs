// SPDX-License-Identifier: MIT
//! IPFS adapter — uploads `.pfimg` artifacts into a local Kubo daemon,
//! returns a directory CID that can be served from any IPFS gateway.
//!
//! Maps `ipfs://<CID>` to a UnixFS directory containing
//! `manifest.json`, `manifest.json.sig`, and one entry per
//! transitively-reachable blob.
//!
//! Push semantics: IPFS is content-addressed, so the resulting CID is
//! determined by what we upload — we can't hit a caller-supplied CID
//! exactly. The CID returned by the daemon is emitted via
//! `tracing::info!(target = "pf-registry::ipfs", cid = ...)` so the
//! caller can pick it up from logs (and `pf push` surfaces it on
//! stdout). The `cid` in the [`ImageRef::Ipfs`] target acts as a
//! re-push hint only.
//!
//! Pull semantics: pass a known directory CID, the adapter walks it
//! and reconstructs the [`crate::registry::LayerSet`].
//!
//! ## Auth / endpoint
//!
//! Default daemon endpoint is `http://127.0.0.1:5001`. Override via
//! `auth["IPFS_API"]` or env `IPFS_API`.

#[cfg(feature = "ipfs-live")]
mod live;
#[cfg(feature = "ipfs-live")]
pub use live::IpfsRegistry;

#[cfg(not(feature = "ipfs-live"))]
use crate::image_ref::ImageRef;
#[cfg(not(feature = "ipfs-live"))]
use crate::registry::{LayerSet, Registry, RegistryError};
#[cfg(not(feature = "ipfs-live"))]
use async_trait::async_trait;
#[cfg(not(feature = "ipfs-live"))]
use pf_core::cas::BlobStore;
#[cfg(not(feature = "ipfs-live"))]
use pf_core::manifest::Manifest;

#[cfg(not(feature = "ipfs-live"))]
#[derive(Debug, Default)]
pub struct IpfsRegistry {
    _endpoint: String,
}

#[cfg(not(feature = "ipfs-live"))]
impl IpfsRegistry {
    /// Construct an IPFS registry stub. Rebuild with
    /// `--features ipfs-live` (default since v1.0.2) for the live path.
    pub fn new(endpoint: String) -> Self {
        Self {
            _endpoint: endpoint,
        }
    }
}

#[cfg(not(feature = "ipfs-live"))]
fn err() -> RegistryError {
    RegistryError::UnsupportedScheme(
        "ipfs:// — pf-registry built without `ipfs-live`. Rebuild with \
         `--features ipfs-live` (default since v1.0.2) to enable IPFS."
            .into(),
    )
}

#[cfg(not(feature = "ipfs-live"))]
#[async_trait]
impl Registry for IpfsRegistry {
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
