// SPDX-License-Identifier: MIT
//! S3-compatible registry adapter (AWS S3 / R2 / MinIO).
//!
//! Maps `s3://<bucket>/<prefix>` to S3 object keys. The on-bucket
//! layout is identical to [`crate::file::FileRegistry`]:
//!
//! ```text
//! <bucket>/<prefix>/
//!   manifest.json
//!   manifest.json.sig
//!   blobs/sha256/<aa>/<aabbccdd…>.zst
//! ```
//!
//! Auth bag honoured keys (passed via `pf_registry::open()` or
//! [`S3Registry::new`]):
//!
//! - `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` — explicit creds.
//!   When absent, the SDK falls back to the standard provider chain
//!   (instance profile, env, `~/.aws/credentials`).
//! - `AWS_REGION` — defaults to `us-east-1`.
//! - `AWS_ENDPOINT_URL` — set to your MinIO/R2 endpoint to redirect
//!   the SDK off AWS.

#[cfg(feature = "s3-live")]
mod live;
#[cfg(feature = "s3-live")]
pub use live::S3Registry;

#[cfg(not(feature = "s3-live"))]
use crate::image_ref::ImageRef;
#[cfg(not(feature = "s3-live"))]
use crate::registry::{LayerSet, Registry, RegistryError};
#[cfg(not(feature = "s3-live"))]
use async_trait::async_trait;
#[cfg(not(feature = "s3-live"))]
use pf_core::cas::BlobStore;
#[cfg(not(feature = "s3-live"))]
use pf_core::manifest::Manifest;

#[cfg(not(feature = "s3-live"))]
#[derive(Debug, Default)]
pub struct S3Registry {
    _auth: std::collections::BTreeMap<String, String>,
}

#[cfg(not(feature = "s3-live"))]
impl S3Registry {
    /// Construct an S3 registry stub. Rebuild with `--features s3-live`
    /// (default since v1.0.2) to enable the live SigV4 path.
    pub fn new(auth: std::collections::BTreeMap<String, String>) -> Self {
        Self { _auth: auth }
    }
}

#[cfg(not(feature = "s3-live"))]
fn err() -> RegistryError {
    RegistryError::UnsupportedScheme(
        "s3:// — pf-registry built without `s3-live`. Rebuild with \
         `--features s3-live` (default since v1.0.2) to enable AWS S3 / R2 / MinIO."
            .into(),
    )
}

#[cfg(not(feature = "s3-live"))]
#[async_trait]
impl Registry for S3Registry {
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
