// SPDX-License-Identifier: MIT
//! # `pf-model`
//!
//! Captures and applies model-weight diffs and implements task-vector merge
//! (TIES + DARE). Phase 0 scaffold only.

#![forbid(unsafe_code)]

/// The kinds of weight-diff this layer can serialize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// Low-rank adapters.
    Lora,
    /// IA³ scaling vectors.
    IA3,
    /// Dense full-finetune delta.
    Full,
    /// In-place test-time training updates.
    InPlaceTtt,
}
