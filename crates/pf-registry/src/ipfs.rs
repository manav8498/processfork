// SPDX-License-Identifier: MIT
//! IPFS adapter — pins manifest + every blob against a local IPFS daemon.
//!
//! Phase 9 ships the trait surface + endpoint plumbing. Live HTTP path
//! lands in v1.0.1 behind `--features ipfs-live`.

use async_trait::async_trait;
use pf_core::cas::BlobStore;
use pf_core::manifest::Manifest;

use crate::image_ref::ImageRef;
use crate::registry::{LayerSet, Registry, RegistryError};

#[derive(Debug, Default)]
pub struct IpfsRegistry {
    /// IPFS daemon API endpoint, default `http://127.0.0.1:5001`.
    _endpoint: String,
}

impl IpfsRegistry {
    pub fn new(endpoint: String) -> Self {
        Self {
            _endpoint: endpoint,
        }
    }
}

fn err() -> RegistryError {
    RegistryError::UnsupportedScheme(
        "ipfs:// — live IPFS adapter lands in v1.0.1 \
         (`--features ipfs-live`)."
            .into(),
    )
}

#[async_trait]
impl Registry for IpfsRegistry {
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
