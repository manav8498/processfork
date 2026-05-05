// SPDX-License-Identifier: MIT
//! # `pf-model`
//!
//! Captures model-weight diffs (LoRA, IA³, full-finetune, in-place TTT)
//! and implements the TIES + DARE task-vector merge from
//! `agent_docs/model-layer.md`.
//!
//! ## What ships in Phase 5 (this commit)
//!
//! - [`diff::ModelDiff`] tagged enum + per-variant payloads.
//! - [`serialize::store_diff`] / [`serialize::load_diff`]: round-trip every
//!   variant through any [`pf_core::cas::BlobStore`].
//! - [`merge::dare`] / [`merge::ties_merge`]: the two task-arithmetic
//!   primitives, tested on small synthetic tensors.
//!
//! ## Wire format
//!
//! For Phase 5 we use a single JSON-typed wire format (`model.diff.v1`) for
//! every variant. Parameter tensors are stored as `Vec<f32>` with shape
//! metadata; safetensors interop lands in Phase 10's vLLM adapter where
//! it's actually needed (no point pulling 50 kloc of safetensors deps now).

#![deny(unsafe_code)]
#![allow(missing_docs)] // documented per-symbol in submodules

pub mod diff;
pub mod merge;
pub mod serialize;

pub use diff::{
    DiffKind, FullDelta, IA3Delta, InPlaceTttDelta, LoraAdapter, LoraDelta, ModelDiff, TttStep,
};
pub use merge::{TiesParams, dare, ties_merge};
pub use serialize::{load_diff, store_diff};
