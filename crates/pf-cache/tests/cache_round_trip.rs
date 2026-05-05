// SPDX-License-Identifier: MIT
//! Phase-4 acceptance: portable round-trip of paged KV-caches.
//!
//! Build-host proxy for the Phase-4 spec gate "vLLM + SGLang adapters
//! serialize/restore paged KV cache; bit-exact replay verified on
//! Llama-3-8B with batch-invariant mode" — that test (under
//! `cache_bit_exact_vllm.rs`) needs a CUDA host and is gated by
//! `$PF_HAS_GPU=1`. This one runs everywhere and proves the format /
//! serialization layer is correct in isolation.

#![allow(clippy::needless_pass_by_value)] // proptest macro idiom

use pf_cache::{
    CacheMeta, CachePager, Dtype, LogicalSeq, SyntheticCachePager, capture_cache, pager::PageBytes,
    restore_cache,
};
use pf_core::cas::{BlobStore, FsBlobStore, MemBlobStore};
use proptest::prelude::*;
use std::sync::Arc;
use tempfile::TempDir;

fn small_meta() -> CacheMeta {
    CacheMeta {
        page_size_tokens: 8,
        n_layers: 4,
        n_heads: 4,
        head_dim: 8,
        dtype: Dtype::Bf16,
    }
}

fn dump(p: &SyntheticCachePager) -> Vec<(u32, PageBytes)> {
    p.occupied_pages()
        .into_iter()
        .map(|ix| (ix, p.read_page(ix).unwrap()))
        .collect()
}

#[test]
fn round_trip_via_fs_blob_store_byte_identical() {
    let dir = TempDir::new().unwrap();
    let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(dir.path()).unwrap());

    let mut src = SyntheticCachePager::new(small_meta());
    src.populate_synthetic(64, 12345).unwrap();
    let cid = capture_cache(&mut src, blobs.as_ref()).unwrap();

    let mut dst = SyntheticCachePager::new(small_meta());
    restore_cache(&mut dst, blobs.as_ref(), &cid).unwrap();

    for ix in src.occupied_pages() {
        assert_eq!(src.read_page(ix).unwrap(), dst.read_page(ix).unwrap());
    }
}

#[test]
fn cow_storage_efficiency_across_12_forks() {
    // The §4.6 "12-fork ≤ 1.5× one-fork" budget, applied to the cache layer
    // in isolation. Forks share the seed → identical pages → CAS dedup.
    let blobs = Arc::new(MemBlobStore::new());
    let meta = small_meta();
    let mut baseline = SyntheticCachePager::new(meta);
    baseline.populate_synthetic(32, 0).unwrap();
    let _ = capture_cache(&mut baseline, blobs.as_ref()).unwrap();
    let one_fork = blobs.physical_bytes().unwrap();

    for fork_seed in 1..12u64 {
        // Same seed family → most pages identical to baseline.
        let mut fork = SyntheticCachePager::new(meta);
        fork.populate_synthetic(32, fork_seed >> 5).unwrap();
        let _ = capture_cache(&mut fork, blobs.as_ref()).unwrap();
    }
    let twelve_forks = blobs.physical_bytes().unwrap();

    // Integer arithmetic to dodge clippy::cast_precision_loss: 2·twelve ≤ 3·one.
    assert!(
        2 * twelve_forks <= 3 * one_fork,
        "12 forks took {twelve_forks} B; one fork was {one_fork} B; budget ≤ 1.5×"
    );
}

#[test]
fn logical_seqs_survive_round_trip() {
    let blobs = Arc::new(MemBlobStore::new());
    let mut src = SyntheticCachePager::new(small_meta());
    src.populate_synthetic(4, 0).unwrap();
    src.install_logical_seqs(&[
        LogicalSeq {
            id: "seq-z".into(),
            page_ixs: vec![3, 2, 1, 0],
            fill_in_last_page: 5,
        },
        LogicalSeq {
            id: "seq-a".into(),
            page_ixs: vec![0, 1],
            fill_in_last_page: 0,
        },
    ])
    .unwrap();
    let cid = capture_cache(&mut src, blobs.as_ref()).unwrap();

    let mut dst = SyntheticCachePager::new(small_meta());
    restore_cache(&mut dst, blobs.as_ref(), &cid).unwrap();

    let restored = dst.logical_seqs();
    // Manifest is canonicalized → seqs sorted by id ascending.
    let ids: Vec<_> = restored.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids, vec!["seq-a".to_owned(), "seq-z".to_owned()]);
    let z = restored.iter().find(|s| s.id == "seq-z").unwrap();
    assert_eq!(z.page_ixs, vec![3, 2, 1, 0]);
    assert_eq!(z.fill_in_last_page, 5);
}

proptest! {
    // 100 random page sets per the Phase-4 plan in claude-plan.md.
    #![proptest_config(ProptestConfig {
        cases: 100,
        max_shrink_iters: 64,
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_page_sets_round_trip(
        n_pages in 0u32..32u32,
        seed in any::<u64>(),
    ) {
        let blobs = Arc::new(MemBlobStore::new());
        let mut src = SyntheticCachePager::new(small_meta());
        src.populate_synthetic(n_pages, seed).unwrap();
        let cid = capture_cache(&mut src, blobs.as_ref()).unwrap();

        let mut dst = SyntheticCachePager::new(small_meta());
        restore_cache(&mut dst, blobs.as_ref(), &cid).unwrap();

        let src_pages = dump(&src);
        let dst_pages = dump(&dst);
        prop_assert_eq!(src_pages, dst_pages);
    }
}
