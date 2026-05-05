// SPDX-License-Identifier: MIT
//! # `pf-registry`
//!
//! Push/pull `.pfimg` artifacts to the four supported registry backends.
//! Phase 0 scaffold only.

#![forbid(unsafe_code)]

/// Supported registry backends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryKind {
    /// Hugging Face Hub.
    HuggingFace,
    /// AWS S3 / R2 / MinIO (any S3-compatible).
    S3,
    /// IPFS (behind feature flag in v1).
    Ipfs,
    /// Local OCI registry (air-gapped).
    LocalOci,
}
