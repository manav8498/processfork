// SPDX-License-Identifier: MIT
//! Content-addressed blob store skeleton.
//!
//! Phase 0: defines the trait surface only. The real on-disk implementation
//! (zstd-compressed, sharded by digest prefix) lands in Phase 1.

use crate::digest::Digest256;
use crate::error::Result;

/// A read/write content-addressed store.
///
/// Implementations MUST guarantee that `get(put(x).await?).await?` round-trips
/// to bytes identical to `x`, and that the returned digest equals
/// [`Digest256::of(&x)`].
pub trait BlobStore: Send + Sync {
    /// Insert `bytes` and return its content digest. Idempotent.
    fn put(&self, bytes: &[u8]) -> Result<Digest256>;

    /// Retrieve the bytes for a previously-stored digest.
    fn get(&self, digest: &Digest256) -> Result<Vec<u8>>;

    /// Cheap existence check.
    fn contains(&self, digest: &Digest256) -> Result<bool>;
}
