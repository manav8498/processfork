// SPDX-License-Identifier: MIT
//! Hugging Face Hub adapter.
//!
//! Maps `hf://<user>/<repo>[:<rev>]` to a HF **dataset** repository
//! (datasets accept arbitrary file content; model repos enforce a
//! safetensors/gguf shape we don't fit).
//!
//! On-repo layout (identical to [`crate::file::FileRegistry`] so a
//! `pf clone hf://…` produces a directory bit-for-bit equal to a
//! `pf clone file://…` of the same image):
//!
//! ```text
//! <repo-root>/
//!   manifest.json
//!   manifest.json.sig
//!   blobs/sha256/<aa>/<aabbccdd…>.zst
//! ```
//!
//! ## Auth
//!
//! `auth["HF_TOKEN"]` is forwarded as `Authorization: Bearer <token>`.
//! Pulls of public repos work without a token; pushes always require one.
//!
//! ## Wire protocol
//!
//! - **Pull**: walk the repo tree at the requested rev
//!   (`GET /api/datasets/{user}/{repo}/tree/{rev}?recursive=true`), then
//!   stream each blob via the resolve endpoint
//!   (`GET /datasets/{user}/{repo}/resolve/{rev}/<path>`). Verify the
//!   manifest signature before parsing.
//! - **Push**: ensure the dataset repo exists
//!   (`POST /api/repos/create`, idempotent, ignores 409), then send
//!   one batched commit
//!   (`POST /api/datasets/{user}/{repo}/commit/{rev}`) carrying the
//!   manifest, the signature, and every transitively-reachable blob —
//!   one HTTP round-trip regardless of blob count, one git history
//!   entry per `pf push`.
//! - **Exists**: HEAD on the resolve URL of `manifest.json`.

#[cfg(feature = "hf-live")]
mod live;
#[cfg(feature = "hf-live")]
pub use live::HfRegistry;

// The stub-mode imports are scoped under the cfg block below so they
// don't trigger `unused_imports` when `hf-live` is on.
#[cfg(not(feature = "hf-live"))]
use crate::image_ref::ImageRef;
#[cfg(not(feature = "hf-live"))]
use crate::registry::{LayerSet, Registry, RegistryError};
#[cfg(not(feature = "hf-live"))]
use async_trait::async_trait;
#[cfg(not(feature = "hf-live"))]
use pf_core::cas::BlobStore;
#[cfg(not(feature = "hf-live"))]
use pf_core::manifest::Manifest;

// ---------------------------------------------------------------------
// Stub when the `hf-live` feature is off: keep the public surface so
// downstream code compiles, but every method returns UnsupportedScheme.
// ---------------------------------------------------------------------
#[cfg(not(feature = "hf-live"))]
#[derive(Debug, Default)]
pub struct HfRegistry {
    _token: Option<String>,
}

#[cfg(not(feature = "hf-live"))]
impl HfRegistry {
    /// Construct an HF registry stub. The `token` is ignored in this
    /// build; rebuild with `--features hf-live` (default since v1.0.2)
    /// to enable the live HTTP path.
    pub fn new(token: Option<String>) -> Self {
        Self { _token: token }
    }
}

#[cfg(not(feature = "hf-live"))]
fn err() -> RegistryError {
    RegistryError::UnsupportedScheme(
        "hf:// — pf-registry built without `hf-live`. Rebuild with \
         `--features hf-live` (default since v1.0.2) to enable HF Hub."
            .into(),
    )
}

#[cfg(not(feature = "hf-live"))]
#[async_trait]
impl Registry for HfRegistry {
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
