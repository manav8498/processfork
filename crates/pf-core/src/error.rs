// SPDX-License-Identifier: MIT
//! Typed error hierarchy for `pf-core`.

use std::io;

/// All errors surfaced by `pf-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Wrapped I/O failure.
    #[error("io: {0}")]
    Io(#[from] io::Error),

    /// JSON manifest (de)serialization failure.
    #[error("manifest: {0}")]
    Manifest(#[from] serde_json::Error),

    /// Hex decoding failed.
    #[error("hex: {0}")]
    Hex(#[from] hex::FromHexError),

    /// A digest was expected to have a specific length / format.
    #[error("invalid digest: {0}")]
    InvalidDigest(String),

    /// CAS round-trip mismatch (data on disk does not hash to its key).
    #[error("CAS integrity check failed for {0}")]
    Integrity(String),

    /// Catch-all for not-yet-implemented features in the Phase-0 scaffold.
    #[error("not yet implemented: {0}")]
    Unimplemented(&'static str),
}

/// `pf-core` result alias.
pub type Result<T> = std::result::Result<T, Error>;
