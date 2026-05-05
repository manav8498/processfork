// SPDX-License-Identifier: MIT
//! Serialize / deserialize a paged KV cache via a [`BlobStore`].
//!
//! Per `agent_docs/cache-layer.md` the K and V buffers of each physical page
//! are content-addressed *independently* so a fork that mutates only V (e.g.
//! a one-token decode step) shares its K page with siblings. The
//! [`pf_core::cas::FsBlobStore`] does the zstd-19 compression at rest, so we
//! pass raw bytes here.

use crate::format::{CacheMeta, LogicalSeq, Page, PageManifest};
use crate::pager::PageBytes;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;

/// Serialize a list of (ix, K-bytes, V-bytes) page tuples + the
/// per-sequence logical mapping into a [`PageManifest`] and persist every
/// blob through `blobs`. Returns the digest of the manifest itself, ready
/// to drop into the `.pfimg` `cache.manifest` field.
pub fn serialize_pages<I>(
    blobs: &dyn BlobStore,
    meta: CacheMeta,
    pages: I,
    logical_seqs: &[LogicalSeq],
) -> pf_core::Result<Digest256>
where
    I: IntoIterator<Item = (u32, PageBytes)>,
{
    let mut manifest = PageManifest::new(meta);
    for (ix, page) in pages {
        if page.k.len() != meta.page_bytes() || page.v.len() != meta.page_bytes() {
            return Err(pf_core::Error::Integrity(format!(
                "serialize_pages ix={ix}: K/V len {}/{} ≠ expected {}",
                page.k.len(),
                page.v.len(),
                meta.page_bytes()
            )));
        }
        let k = blobs.put(&page.k)?;
        let v = blobs.put(&page.v)?;
        manifest.pages.push(Page { ix, k, v });
    }
    manifest.logical_seqs = logical_seqs.to_vec();
    manifest.canonicalize();
    blobs.put(&serde_json::to_vec(&manifest)?)
}

/// Inverse of [`serialize_pages`]. Loads the [`PageManifest`] at `digest`,
/// fetches every K/V blob, and yields `(ix, PageBytes)` pairs in canonical
/// order along with the logical-seqs and metadata needed to rebuild the
/// engine's page table.
pub fn deserialize_pages(
    blobs: &dyn BlobStore,
    digest: &Digest256,
) -> pf_core::Result<DeserializedCache> {
    let bytes = blobs.get(digest)?;
    let manifest: PageManifest = serde_json::from_slice(&bytes)?;
    if manifest.layout != crate::format::LAYOUT_V1 {
        return Err(pf_core::Error::Integrity(format!(
            "expected layout {}, got {}",
            crate::format::LAYOUT_V1,
            manifest.layout
        )));
    }
    let mut pages = Vec::with_capacity(manifest.pages.len());
    for p in &manifest.pages {
        let k = blobs.get(&p.k)?;
        let v = blobs.get(&p.v)?;
        if k.len() != manifest.meta.page_bytes() || v.len() != manifest.meta.page_bytes() {
            return Err(pf_core::Error::Integrity(format!(
                "deserialize_pages ix={}: K/V len {}/{} ≠ expected {}",
                p.ix,
                k.len(),
                v.len(),
                manifest.meta.page_bytes()
            )));
        }
        pages.push((p.ix, PageBytes { k, v }));
    }
    Ok(DeserializedCache {
        meta: manifest.meta,
        pages,
        logical_seqs: manifest.logical_seqs,
    })
}

/// Output of [`deserialize_pages`].
#[derive(Debug)]
pub struct DeserializedCache {
    /// Static metadata exactly as it was at capture time.
    pub meta: CacheMeta,
    /// `(physical_ix, page_bytes)` pairs in canonical (ix-ascending) order.
    pub pages: Vec<(u32, PageBytes)>,
    /// Logical sequences in canonical (id-ascending) order.
    pub logical_seqs: Vec<LogicalSeq>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Dtype;
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

    fn dump(pager: &SyntheticCachePager) -> Vec<(u32, PageBytes)> {
        pager
            .occupied_pages()
            .into_iter()
            .map(|ix| (ix, pager.read_page(ix).unwrap()))
            .collect()
    }

    #[test]
    fn round_trip_byte_identical() {
        let mut p = SyntheticCachePager::new(small_meta());
        p.populate_synthetic(8, 42).unwrap();
        let blobs = MemBlobStore::new();
        let original = dump(&p);
        let cid = serialize_pages(
            &blobs,
            p.meta(),
            original.iter().cloned(),
            &p.logical_seqs(),
        )
        .unwrap();

        let back = deserialize_pages(&blobs, &cid).unwrap();
        assert_eq!(back.meta, p.meta());
        assert_eq!(back.pages, original);
        assert_eq!(back.logical_seqs, p.logical_seqs());
    }

    #[test]
    fn cow_dedupes_identical_pages_across_two_pagers() {
        let mut a = SyntheticCachePager::new(small_meta());
        let mut b = SyntheticCachePager::new(small_meta());
        a.populate_synthetic(8, 1).unwrap();
        b.populate_synthetic(8, 1).unwrap(); // same seed → identical pages

        let blobs = MemBlobStore::new();
        let _ = serialize_pages(&blobs, a.meta(), dump(&a), &a.logical_seqs()).unwrap();
        let after_a = blobs.physical_bytes().unwrap();
        let _ = serialize_pages(&blobs, b.meta(), dump(&b), &b.logical_seqs()).unwrap();
        let after_b = blobs.physical_bytes().unwrap();

        // Two pagers with byte-identical pages → only the second manifest
        // (and the trivially-different logical-seq id) should grow the store.
        let growth = after_b - after_a;
        let allowance = 1024;
        assert!(
            growth < allowance,
            "second pager grew CAS by {growth} B (>{allowance}); CoW failing"
        );
    }

    #[test]
    fn cow_partial_divergence_only_stores_diff() {
        // Two pagers that differ in ONE page should add ~2 page-blobs to CAS
        // (one new K, one new V), not 8 × 2.
        let meta = small_meta();
        let mut a = SyntheticCachePager::new(meta);
        let mut b = SyntheticCachePager::new(meta);
        a.populate_synthetic(8, 1).unwrap();
        b.populate_synthetic(8, 1).unwrap();

        // Mutate page 3 in b only.
        let bad = PageBytes {
            k: vec![0xAA; meta.page_bytes()],
            v: vec![0xBB; meta.page_bytes()],
        };
        b.write_page(3, &bad).unwrap();

        let blobs = MemBlobStore::new();
        let _ = serialize_pages(&blobs, meta, dump(&a), &a.logical_seqs()).unwrap();
        let after_a = blobs.physical_bytes().unwrap();
        let _ = serialize_pages(&blobs, meta, dump(&b), &b.logical_seqs()).unwrap();
        let after_b = blobs.physical_bytes().unwrap();

        let growth = after_b - after_a;
        // Generous: 2 page blobs (K+V) + manifest JSON.
        let cap = 2 * (meta.page_bytes() as u64) + 2048;
        assert!(growth < cap, "growth {growth} > cap {cap}");
    }

    #[test]
    fn deserialize_rejects_wrong_layout() {
        let blobs = MemBlobStore::new();
        let bogus = serde_json::json!({
            "layout": "some-other-layout",
            "page_size_tokens": 4, "n_layers": 2, "n_heads": 2, "head_dim": 4,
            "dtype": "bf16",
            "pages": [], "logical_seqs": []
        });
        let cid = blobs.put(&serde_json::to_vec(&bogus).unwrap()).unwrap();
        let err = deserialize_pages(&blobs, &cid).unwrap_err();
        assert!(matches!(err, pf_core::Error::Integrity(_)));
    }

    #[test]
    fn page_canonicalized_order_in_manifest() {
        // Insertion order shouldn't matter; canonicalize() sorts by ix.
        let meta = small_meta();
        let mut pager = SyntheticCachePager::new(meta);
        pager.populate_synthetic(4, 0).unwrap();
        let blobs = MemBlobStore::new();
        // Reverse iteration order on input.
        let mut reversed = dump(&pager);
        reversed.reverse();
        let cid = serialize_pages(&blobs, meta, reversed, &pager.logical_seqs()).unwrap();
        let back = deserialize_pages(&blobs, &cid).unwrap();
        let ixs: Vec<u32> = back.pages.iter().map(|(i, _)| *i).collect();
        assert_eq!(ixs, vec![0, 1, 2, 3]);
    }
}
