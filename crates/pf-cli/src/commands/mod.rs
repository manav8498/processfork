// SPDX-License-Identifier: MIT
//! One module per `pf` subcommand. Each module exposes:
//!   - `Args`: clap-derive struct (defines the subcommand's flags).
//!   - `run(store_root, args) -> anyhow::Result<()>`: wired to layer crates.
//
// Standard CLI ergonomics that clippy quibbles with:
//
// - `pub fn run(_: &Path, args: Args)` is the canonical clap-derive
//   handler shape; passing Args by value is intentional (it's then moved
//   into the body without further composition).
// - All `run` fns return `anyhow::Result<()>` for dispatcher-table
//   uniformity — even `completions`, where it's structurally Ok.
// - Status's MiB display does an `as f64` cast on the byte count; we
//   never expect 2^52-byte stores in practice.

#![allow(
    clippy::needless_pass_by_value,
    clippy::unnecessary_wraps,
    clippy::cast_precision_loss
)]

pub mod checkout;
pub mod completions;
pub mod diff;
pub mod fork;
pub mod gc;
pub mod log;
pub mod merge;
pub mod merge_finalize;
pub mod merge_resolve;
pub mod snapshot;
pub mod status;
pub mod stub;
pub mod verify;

/// Typed errors that the top-level `main` maps to specific exit codes per
/// `agent_docs/cli-spec.md`.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// `1` — user-recoverable bad input.
    #[error("{0}")]
    BadInput(String),
    /// `2` — subcommand is implemented in a later phase.
    #[error("{0}")]
    NotYetImplemented(String),
    /// `3` — merge conflict needs human resolution.
    #[error("{0}")]
    MergeConflict(String),
    /// `4` — integrity failure (CAS hash mismatch, signature, …).
    #[error("{0}")]
    Integrity(String),
}

impl CliError {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::BadInput(_) => 1,
            Self::NotYetImplemented(_) => 2,
            Self::MergeConflict(_) => 3,
            Self::Integrity(_) => 4,
        }
    }
}
