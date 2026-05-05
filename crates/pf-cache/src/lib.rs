// SPDX-License-Identifier: MIT
//! # `pf-cache`
//!
//! Paged KV-cache capture, content-addressing per page (CoW across forks),
//! and a [`CachePager`] trait that the per-engine adapters implement.
//!
//! See `agent_docs/cache-layer.md` for the spec and
//! `.claude/skills/kvcache-format/SKILL.md` for the page-out / page-in
//! pseudo-code. The on-disk format is `paged-batchinvariant-v1`.
//!
//! ## What ships in Phase 4 (this commit)
//!
//! - [`format::PageManifest`]: the wire-format struct mirrored from the spec.
//! - [`serialize::serialize_pages`] / [`serialize::deserialize_pages`]:
//!   portable round-trip via the [`pf_core::cas::BlobStore`] trait, no GPU.
//! - [`pager::CachePager`]: the engine-agnostic interface every adapter
//!   implements (vLLM, SGLang, …).
//! - [`pager::SyntheticCachePager`]: in-memory implementation used by every
//!   test in this crate. Lets us prove serialize+restore round-trip without
//!   booting an inference engine.
//! - [`capture::capture_cache`] / [`capture::restore_cache`]: high-level
//!   one-shot helpers that the snapshotter calls.
//!
//! ## Bit-exact replay
//!
//! Bit-exact restore requires batch-invariant kernels (vLLM
//! `--enforce-deterministic`, SGLang `--deterministic-mode`). The CUDA-host
//! integration test (`tests/cache_bit_exact_vllm.rs`) is gated behind
//! `$PF_HAS_GPU=1`; the in-process round-trip in `tests/cache_round_trip.rs`
//! is the build-host proxy and runs everywhere.

#![deny(unsafe_code)]
#![allow(missing_docs)] // documented per-symbol in submodules

pub mod capture;
pub mod format;
pub mod pager;
pub mod serialize;

pub use capture::{capture_cache, restore_cache};
pub use format::{CacheMeta, Dtype, LAYOUT_V1, LogicalSeq, Page, PageManifest};
pub use pager::{CachePager, SyntheticCachePager};
pub use serialize::{deserialize_pages, serialize_pages};
