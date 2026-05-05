// SPDX-License-Identifier: MIT
//! Hugging Face Hub adapter.
//!
//! Phase 9 ships the trait surface + URL parsing + auth-token plumbing.
//! The live HTTP path lands behind the `hf-live` feature flag in v1.0.1
//! (needs `HF_TOKEN`); test infrastructure pre-built so the operator
//! drops creds into `$HF_TOKEN` and runs `cargo test --features hf-live`.

use async_trait::async_trait;
use pf_core::cas::BlobStore;
use pf_core::manifest::Manifest;

use crate::image_ref::ImageRef;
use crate::registry::{LayerSet, Registry, RegistryError};

#[derive(Debug, Default)]
pub struct HfRegistry {
    /// `HF_TOKEN`. `None` means anonymous read-only mode.
    _token: Option<String>,
}

impl HfRegistry {
    pub fn new(token: Option<String>) -> Self {
        Self { _token: token }
    }
}

fn err() -> RegistryError {
    RegistryError::UnsupportedScheme(
        "hf:// — live HF Hub adapter lands in v1.0.1 (`--features hf-live`). \
         The trait surface and URL parsing are stable; the HTTP path is the \
         deferred work."
            .into(),
    )
}

#[async_trait]
impl Registry for HfRegistry {
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
