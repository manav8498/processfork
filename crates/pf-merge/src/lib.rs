// SPDX-License-Identifier: MIT
//! # `pf-merge`
//!
//! Typed three-way merge across all four ProcessFork layers + the trace.
//! See `agent_docs/merge-protocol.md` for the spec.
//!
//! ## What ships in Phase 6 (this commit)
//!
//! - [`ancestor::find_lca`]: BFS lowest-common-ancestor on the manifest
//!   parents DAG. Multi-parent (octopus) merges error in v1.
//! - [`trace::merge_trace`]: pluggable [`trace::Summarizer`] interface +
//!   `StubSummarizer` test impl. Live Claude API call gated by the
//!   `live-summarizer` feature flag.
//! - [`world::merge_world`]: three-way file diff over the
//!   [`pf_world::FsTree`] format. Conflicts produce real `<<<<<<<`-marker
//!   blobs and a structured [`world::WorldConflict`] list.
//! - [`effects::merge_effects`]: emits an `effects.merged.v1` blob that
//!   references both parent ledgers + the replay-class policy. Honours
//!   the strict default (never replay irreversible).
//! - [`model::merge_model`]: variant-dispatch wrapper around
//!   `pf_model::ties_merge` + `pf_model::dare`. LoRA merges by
//!   `(layer_id, matrix)`; Full merges by parameter name; IA³ merges
//!   by `(layer, matrix)`; InPlaceTtt is concatenated.
//! - [`engine::merge`]: top-level orchestrator producing a new manifest.

#![deny(unsafe_code)]
#![allow(missing_docs)]
// documented per-symbol in submodules
// The world-layer 3-way merge is one big match on the 27-cell decision
// table; splitting it doesn't improve clarity. The test fixtures use
// short names + small integer casts that are safe in context.
#![allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::cast_possible_truncation
)]

pub mod ancestor;
pub mod effects;
pub mod engine;
pub mod model;
pub mod trace;
pub mod world;

pub use ancestor::{AncestorError, find_lca};
pub use effects::{MergedEffectsLedger, ReplayPolicySummary, merge_effects};
pub use engine::{MergeOptions, MergeReport, merge};
pub use model::{ModelMergeOutcome, merge_model};
pub use trace::{StubSummarizer, Summarizer, TraceMergeOutcome, merge_trace};
pub use world::{WorldConflict, WorldMergeOutcome, merge_world};

/// What the merge engine ended up doing for a given layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Clean: no conflicts, target updated.
    Clean,
    /// Conflicts surfaced and require user intervention.
    Conflicted,
    /// Skipped (e.g. effects layer with `--no-replay-effects`).
    Skipped,
}
