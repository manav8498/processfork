// SPDX-License-Identifier: MIT
//! S3-compatible adapter (AWS / R2 / MinIO).
//!
//! Phase 9 ships the trait surface + URL parsing + AWS-creds plumbing.
//! Live SigV4 path lands in v1.0.1 behind `--features s3-live`.

use std::collections::BTreeMap;

use async_trait::async_trait;
use pf_core::cas::BlobStore;
use pf_core::manifest::Manifest;

use crate::image_ref::ImageRef;
use crate::registry::{LayerSet, Registry, RegistryError};

#[derive(Debug, Default)]
pub struct S3Registry {
    /// AWS-style auth bag: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
    /// `AWS_REGION`, `AWS_ENDPOINT_URL` (R2 / MinIO).
    _auth: BTreeMap<String, String>,
}

impl S3Registry {
    pub fn new(auth: BTreeMap<String, String>) -> Self {
        Self { _auth: auth }
    }
}

fn err() -> RegistryError {
    RegistryError::UnsupportedScheme(
        "s3:// — live S3 adapter lands in v1.0.1 (`--features s3-live`). \
         The trait surface and URL parsing are stable; the SigV4 wire \
         is the deferred work."
            .into(),
    )
}

#[async_trait]
impl Registry for S3Registry {
    async fn push(
        &self,
        _target: &ImageRef,
        _manifest: &Manifest,
        _blobs: &dyn BlobStore,
    ) -> Result<(), RegistryError> {
        Err(err())
    }
    async fn pull(&self, _source: &ImageRef) -> Result<LayerSet, RegistryError> {
        Err(err())
    }
    async fn exists(&self, _source: &ImageRef) -> Result<bool, RegistryError> {
        Err(err())
    }
}
