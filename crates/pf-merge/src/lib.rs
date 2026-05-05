// SPDX-License-Identifier: MIT
//! # `pf-merge`
//!
//! Typed three-way merge across all four layers. See
//! `agent_docs/merge-protocol.md` for the spec. Phase 0 scaffold only.

#![forbid(unsafe_code)]

/// What the merge engine ended up doing for a given layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Clean: no conflicts, target updated.
    Clean,
    /// Conflicts surfaced and require user intervention.
    Conflicted,
    /// Skipped (e.g., effects layer with `--no-replay-effects`).
    Skipped,
}
