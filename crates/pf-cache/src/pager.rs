// SPDX-License-Identifier: MIT
//! Engine-agnostic paged-cache interface.
//!
//! Every adapter (vLLM, SGLang, …) implements [`CachePager`]. The
//! [`SyntheticCachePager`] in this module is the in-process implementation
//! that lets the test suite (and the macOS build host) round-trip the cache
//! layer end-to-end without booting an inference engine.

use crate::format::{CacheMeta, LogicalSeq};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;

/// One physical (K, V) page as a pair of opaque byte buffers, both of length
/// `meta.page_bytes()`. We never interpret the bytes — they're whatever the
/// engine handed us.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageBytes {
    /// Raw K-tensor bytes for this page.
    pub k: Vec<u8>,
    /// Raw V-tensor bytes for this page.
    pub v: Vec<u8>,
}

/// Engine-agnostic paged-cache interface. Implementations are *not* required
/// to be thread-safe for `read_page` / `write_page` — the snapshot
/// orchestrator holds a `&mut self` for the duration of capture/restore.
pub trait CachePager: Send {
    /// Static cache metadata (page size, layer count, dtype, …).
    fn meta(&self) -> CacheMeta;

    /// Pause the engine so the page table is stable. May be a no-op for
    /// adapters that already hold a write-lock during snapshot.
    fn pause(&mut self) -> pf_core::Result<()>;

    /// Resume the engine after pause / restore.
    fn resume(&mut self) -> pf_core::Result<()>;

    /// List physical-page indices that currently hold valid data.
    fn occupied_pages(&self) -> Vec<u32>;

    /// Snapshot of every live logical sequence's page-table mapping.
    fn logical_seqs(&self) -> Vec<LogicalSeq>;

    /// Read one (K, V) page out of the engine. Implementations return raw
    /// bytes of length `meta.page_bytes()` for each.
    fn read_page(&self, ix: u32) -> pf_core::Result<PageBytes>;

    /// Allocate `n` fresh physical-page slots and return their indices in
    /// the order they should be filled. Restore writes pages in this order.
    fn allocate_pages(&mut self, n: usize) -> pf_core::Result<Vec<u32>>;

    /// Write one (K, V) page into the engine at the given physical-page
    /// index. The index MUST have been returned by a recent
    /// [`Self::allocate_pages`] call.
    fn write_page(&mut self, ix: u32, page: &PageBytes) -> pf_core::Result<()>;

    /// Re-install the per-request logical-seq → page-list mapping after
    /// restore. Called once, after all pages have been written.
    fn install_logical_seqs(&mut self, seqs: &[LogicalSeq]) -> pf_core::Result<()>;
}

// ----------------------- SyntheticCachePager -----------------------

/// In-memory [`CachePager`] used by every test in this crate and by
/// `examples/02-twelve-way-parallel/` (when that lands).
///
/// Stores pages in a `BTreeMap<u32, PageBytes>` keyed by physical-page index.
/// `allocate_pages` returns monotonically increasing indices; `write_page`
/// inserts; `read_page` reads.
///
/// `Clone`-able — the inner state is `Arc<Mutex<…>>` so two clones share the
/// same logical engine. Useful for "fork the in-memory engine; mutate one
/// branch; serialize each; assert dedup."
#[derive(Clone, Debug)]
pub struct SyntheticCachePager {
    inner: Arc<Mutex<SynthInner>>,
    meta: CacheMeta,
}

#[derive(Debug)]
struct SynthInner {
    pages: BTreeMap<u32, PageBytes>,
    logical_seqs: Vec<LogicalSeq>,
    next_page_ix: u32,
    paused: bool,
}

impl SyntheticCachePager {
    /// Construct an empty engine with the given static metadata.
    #[must_use]
    pub fn new(meta: CacheMeta) -> Self {
        Self {
            meta,
            inner: Arc::new(Mutex::new(SynthInner {
                pages: BTreeMap::new(),
                logical_seqs: Vec::new(),
                next_page_ix: 0,
                paused: false,
            })),
        }
    }

    /// Pre-populate the engine with `n_pages` deterministic pages and one
    /// logical sequence covering them all. Useful for tests / benches.
    ///
    /// `seed` mixes into the per-page fill so different seeds produce
    /// different pages (driving CAS divergence) while the same seed produces
    /// byte-identical pages (driving CAS dedup).
    pub fn populate_synthetic(&mut self, n_pages: u32, seed: u64) -> pf_core::Result<()> {
        let mut g = self.inner.lock();
        let bytes = self.meta.page_bytes();
        for i in 0..n_pages {
            let mut k = vec![0u8; bytes];
            let mut v = vec![0u8; bytes];
            fill_deterministic(&mut k, seed ^ u64::from(i) ^ 0xCACE_CAFE_CAFE);
            fill_deterministic(&mut v, seed ^ u64::from(i) ^ 0xC0DE_C0DE_C0DE_C0DE);
            g.pages.insert(i, PageBytes { k, v });
        }
        g.next_page_ix = n_pages;
        g.logical_seqs.push(LogicalSeq {
            id: format!("seq-seed-{seed:016x}"),
            page_ixs: (0..n_pages).collect(),
            fill_in_last_page: self.meta.page_size_tokens / 2,
        });
        Ok(())
    }
}

/// SplitMix64 — the same generator we use in `pf-core::fixture`. Cheap,
/// deterministic, no crate dep.
fn fill_deterministic(buf: &mut [u8], seed: u64) {
    let mut s = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    for chunk in buf.chunks_mut(8) {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let bytes = z.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

impl CachePager for SyntheticCachePager {
    fn meta(&self) -> CacheMeta {
        self.meta
    }

    fn pause(&mut self) -> pf_core::Result<()> {
        self.inner.lock().paused = true;
        Ok(())
    }

    fn resume(&mut self) -> pf_core::Result<()> {
        self.inner.lock().paused = false;
        Ok(())
    }

    fn occupied_pages(&self) -> Vec<u32> {
        self.inner.lock().pages.keys().copied().collect()
    }

    fn logical_seqs(&self) -> Vec<LogicalSeq> {
        self.inner.lock().logical_seqs.clone()
    }

    fn read_page(&self, ix: u32) -> pf_core::Result<PageBytes> {
        self.inner
            .lock()
            .pages
            .get(&ix)
            .cloned()
            .ok_or_else(|| pf_core::Error::Integrity(format!("no page at ix {ix}")))
    }

    fn allocate_pages(&mut self, n: usize) -> pf_core::Result<Vec<u32>> {
        let mut g = self.inner.lock();
        let start = g.next_page_ix;
        let n_u32 = u32::try_from(n).map_err(|e| {
            pf_core::Error::Integrity(format!("allocate_pages: {n} too large: {e}"))
        })?;
        g.next_page_ix = g.next_page_ix.checked_add(n_u32).ok_or_else(|| {
            pf_core::Error::Integrity("allocate_pages: page-index overflow".into())
        })?;
        Ok((start..start + n_u32).collect())
    }

    fn write_page(&mut self, ix: u32, page: &PageBytes) -> pf_core::Result<()> {
        let expected = self.meta.page_bytes();
        if page.k.len() != expected || page.v.len() != expected {
            return Err(pf_core::Error::Integrity(format!(
                "write_page ix={ix}: K/V len {}/{} ≠ expected {expected}",
                page.k.len(),
                page.v.len()
            )));
        }
        self.inner.lock().pages.insert(ix, page.clone());
        Ok(())
    }

    fn install_logical_seqs(&mut self, seqs: &[LogicalSeq]) -> pf_core::Result<()> {
        self.inner.lock().logical_seqs = seqs.to_vec();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Dtype;

    fn small_meta() -> CacheMeta {
        // Tiny by design — keeps tests fast.
        CacheMeta {
            page_size_tokens: 4,
            n_layers: 2,
            n_heads: 2,
            head_dim: 4,
            dtype: Dtype::Bf16,
        }
    }

    #[test]
    fn populate_then_read_round_trips_bytes() {
        let mut p = SyntheticCachePager::new(small_meta());
        p.populate_synthetic(8, 42).unwrap();
        let occ = p.occupied_pages();
        assert_eq!(occ, (0..8).collect::<Vec<_>>());
        let page0 = p.read_page(0).unwrap();
        let page0_again = p.read_page(0).unwrap();
        assert_eq!(page0, page0_again, "read_page deterministic");
    }

    #[test]
    fn same_seed_produces_byte_identical_pages() {
        let mut a = SyntheticCachePager::new(small_meta());
        let mut b = SyntheticCachePager::new(small_meta());
        a.populate_synthetic(4, 7).unwrap();
        b.populate_synthetic(4, 7).unwrap();
        for ix in 0..4 {
            assert_eq!(a.read_page(ix).unwrap(), b.read_page(ix).unwrap());
        }
    }

    #[test]
    fn different_seed_diverges() {
        let mut a = SyntheticCachePager::new(small_meta());
        let mut b = SyntheticCachePager::new(small_meta());
        a.populate_synthetic(4, 7).unwrap();
        b.populate_synthetic(4, 9).unwrap();
        assert_ne!(a.read_page(0).unwrap(), b.read_page(0).unwrap());
    }

    #[test]
    fn write_page_validates_length() {
        let mut p = SyntheticCachePager::new(small_meta());
        let ixs = p.allocate_pages(1).unwrap();
        let bad = PageBytes {
            k: vec![0u8; 1],
            v: vec![0u8; 1],
        };
        assert!(p.write_page(ixs[0], &bad).is_err());
    }

    #[test]
    fn allocate_pages_returns_monotonic_indices() {
        let mut p = SyntheticCachePager::new(small_meta());
        let a = p.allocate_pages(3).unwrap();
        let b = p.allocate_pages(2).unwrap();
        assert_eq!(a, vec![0, 1, 2]);
        assert_eq!(b, vec![3, 4]);
    }
}
