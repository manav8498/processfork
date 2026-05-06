// SPDX-License-Identifier: MIT
//! # `pf-registry`
//!
//! Push / pull `.pfimg` artifacts to one of five registry backends. See
//! `agent_docs/registry-spec.md` for the URL-scheme table and the auth
//! conventions.
//!
//! ## What ships in Phase 9 (this commit)
//!
//! - [`image_ref::ImageRef`]: parser for `file://`, `hf://`, `s3://`,
//!   `ipfs://`, `oci://` URLs.
//! - [`registry::Registry`] trait + [`registry::RegistryError`].
//! - [`file::FileRegistry`]: filesystem-backed registry for tests and
//!   air-gapped use. Round-trips a full `.pfimg` (manifest + every
//!   referenced layer blob) into a target directory.
//! - [`hf::HfRegistry`], [`s3::S3Registry`], [`ipfs::IpfsRegistry`]:
//!   trait surface + URL parsing + `not_yet_implemented` push/pull
//!   gated behind feature flags. Real implementations land in v1.0.1.
//! - [`sign::ManifestSignature`] / [`sign::sign_manifest`] /
//!   [`sign::verify_manifest`]: cosign-shaped signing for the v1
//!   self-signed mode. Sigstore Fulcio integration is feature-gated
//!   (`sigstore-keyless`) for v1.1.

#![deny(unsafe_code)]
#![allow(missing_docs)]
// documented per-symbol in submodules
// async_trait emits future-proxy types that clippy quibbles with;
// these allows scope to the registry crate only.
// `collapsible_if` was introduced in Rust 1.88's clippy; the
// transitive-blob walker (registry.rs) deliberately uses staircase
// `if let Ok(...)` blocks for readability over `let-else`.
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions,
    clippy::collapsible_if,
    clippy::collapsible_match
)]

pub mod file;
pub mod hf;
pub mod image_ref;
pub mod ipfs;
pub mod oci;
pub mod registry;
pub mod s3;
pub mod sign;

pub use file::FileRegistry;
pub use hf::HfRegistry;
pub use image_ref::{ImageRef, ImageRefError};
pub use ipfs::IpfsRegistry;
pub use oci::OciRegistry;
pub use registry::{LayerSet, Registry, RegistryError};
pub use s3::S3Registry;
pub use sign::{ManifestSignature, sign_manifest, verify_manifest};

/// Construct a [`Registry`] for a given URL scheme. Returns
/// `Err(RegistryError::UnsupportedScheme)` for schemes whose
/// implementation lives behind a feature flag we weren't built with.
///
/// `auth` is a key/value bag of credentials (e.g. `{"HF_TOKEN": "..."}`);
/// adapters consume the entries they recognise and ignore the rest.
pub fn open(
    image_ref: &ImageRef,
    auth: &std::collections::BTreeMap<String, String>,
) -> Result<Box<dyn Registry>, RegistryError> {
    match image_ref {
        ImageRef::File { .. } => Ok(Box::new(FileRegistry::new())),
        ImageRef::Hf { .. } => Ok(Box::new(HfRegistry::new(auth.get("HF_TOKEN").cloned()))),
        ImageRef::S3 { .. } => Ok(Box::new(S3Registry::new(auth.clone()))),
        ImageRef::Ipfs { .. } => Ok(Box::new(IpfsRegistry::new(
            auth.get("IPFS_API")
                .cloned()
                .unwrap_or_else(|| "http://127.0.0.1:5001".into()),
        ))),
        ImageRef::Oci { .. } => Ok(Box::new(OciRegistry::new(auth.clone()))),
    }
}
