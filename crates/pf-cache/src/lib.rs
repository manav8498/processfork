// SPDX-License-Identifier: MIT
//! # `pf-cache`
//!
//! Paged KV-cache capture, content-addressing per page (CoW across forks),
//! and adapter glue for vLLM ≥0.10 and SGLang ≥0.5 in batch-invariant mode.
//! Phase 0 scaffold only.

#![forbid(unsafe_code)]

/// On-disk KV-cache layout identifiers we know how to (de)serialize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheLayout {
    /// Default v1 — paged, batch-invariant, deterministic on restore.
    PagedBatchInvariantV1,
}
