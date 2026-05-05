// SPDX-License-Identifier: MIT
//! Criterion bench: paged KV-cache serialize → deserialize round-trip.

use criterion::{Criterion, criterion_group, criterion_main};
use pf_cache::{CacheMeta, Dtype, SyntheticCachePager, capture_cache, restore_cache};
use pf_core::cas::MemBlobStore;

fn small_meta() -> CacheMeta {
    CacheMeta {
        page_size_tokens: 8,
        n_layers: 4,
        n_heads: 4,
        head_dim: 8,
        dtype: Dtype::Bf16,
    }
}

fn bench_capture_then_restore(c: &mut Criterion) {
    let meta = small_meta();
    let mut pager = SyntheticCachePager::new(meta);
    pager.populate_synthetic(64, 0).unwrap();

    c.bench_function("cache_capture_64_pages", |b| {
        b.iter(|| {
            let blobs = MemBlobStore::new();
            let _ = capture_cache(&mut pager, &blobs).unwrap();
        });
    });

    let blobs = MemBlobStore::new();
    let cid = capture_cache(&mut pager, &blobs).unwrap();

    c.bench_function("cache_restore_64_pages", |b| {
        b.iter(|| {
            let mut dst = SyntheticCachePager::new(meta);
            restore_cache(&mut dst, &blobs, &cid).unwrap();
        });
    });
}

criterion_group!(benches, bench_capture_then_restore);
criterion_main!(benches);
