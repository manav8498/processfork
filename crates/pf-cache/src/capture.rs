// SPDX-License-Identifier: MIT
//! High-level capture / restore one-shot helpers.
//!
//! Drives a [`CachePager`] through pause → walk-pages → serialize → resume,
//! and the inverse on restore. Adapter authors normally call these instead
//! of touching [`crate::serialize`] directly.

use crate::pager::CachePager;
use crate::serialize::{deserialize_pages, serialize_pages};
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;

/// Capture the entire live state of `pager` into the supplied `blobs` store.
///
/// Returns the digest of the [`crate::format::PageManifest`] blob; this is
/// the value that goes into the `.pfimg` manifest's `cache.manifest` field.
///
/// Pauses the pager for the duration of the capture and resumes on the way
/// out — even if a page read errors midway through.
pub fn capture_cache(
    pager: &mut dyn CachePager,
    blobs: &dyn BlobStore,
) -> pf_core::Result<Digest256> {
    pager.pause()?;
    // Use a guard pattern to guarantee resume() runs.
    let result = (|| {
        let meta = pager.meta();
        let occupied = pager.occupied_pages();
        let logical_seqs = pager.logical_seqs();
        let mut captured = Vec::with_capacity(occupied.len());
        for ix in occupied {
            let bytes = pager.read_page(ix)?;
            captured.push((ix, bytes));
        }
        serialize_pages(blobs, meta, captured, &logical_seqs)
    })();
    let resume = pager.resume();
    let cid = result?;
    resume?;
    Ok(cid)
}

/// Restore the cache described by `manifest_digest` into `pager`. The pager
/// must be empty (no occupied pages) and the same engine config as the one
/// that produced the snapshot. Mismatches surface as
/// [`pf_core::Error::Integrity`].
pub fn restore_cache(
    pager: &mut dyn CachePager,
    blobs: &dyn BlobStore,
    manifest_digest: &Digest256,
) -> pf_core::Result<()> {
    let restored = deserialize_pages(blobs, manifest_digest)?;
    if restored.meta != pager.meta() {
        return Err(pf_core::Error::Integrity(format!(
            "cache meta mismatch on restore: got {:?}, pager is {:?}",
            restored.meta,
            pager.meta()
        )));
    }
    pager.pause()?;
    let result = (|| {
        // Allocate fresh slots; then, for each captured page, write into the
        // pager at the captured *original* physical-page index — adapters
        // are free to remap, but the synthetic pager assumes ix-stability.
        let _ = pager.allocate_pages(restored.pages.len())?;
        for (ix, bytes) in &restored.pages {
            pager.write_page(*ix, bytes)?;
        }
        pager.install_logical_seqs(&restored.logical_seqs)
    })();
    let resume = pager.resume();
    result?;
    resume
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{CacheMeta, Dtype};
    use crate::pager::{CachePager, SyntheticCachePager};
    use pf_core::cas::MemBlobStore;

    fn small_meta() -> CacheMeta {
        CacheMeta {
            page_size_tokens: 4,
            n_layers: 2,
            n_heads: 2,
            head_dim: 4,
            dtype: Dtype::Bf16,
        }
    }

    #[test]
    fn capture_then_restore_into_fresh_pager() {
        let mut src = SyntheticCachePager::new(small_meta());
        src.populate_synthetic(8, 99).unwrap();
        let blobs = MemBlobStore::new();
        let cid = capture_cache(&mut src, &blobs).unwrap();

        let mut dst = SyntheticCachePager::new(small_meta());
        restore_cache(&mut dst, &blobs, &cid).unwrap();

        // Every page byte-identical?
        for ix in src.occupied_pages() {
            assert_eq!(src.read_page(ix).unwrap(), dst.read_page(ix).unwrap());
        }
        assert_eq!(src.logical_seqs(), dst.logical_seqs());
    }

    #[test]
    fn restore_into_mismatched_meta_errors() {
        let mut src = SyntheticCachePager::new(small_meta());
        src.populate_synthetic(2, 0).unwrap();
        let blobs = MemBlobStore::new();
        let cid = capture_cache(&mut src, &blobs).unwrap();

        let mut wider = small_meta();
        wider.head_dim = 8; // diverges
        let mut dst = SyntheticCachePager::new(wider);
        let err = restore_cache(&mut dst, &blobs, &cid).unwrap_err();
        assert!(matches!(err, pf_core::Error::Integrity(_)));
    }
}
