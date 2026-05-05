// SPDX-License-Identifier: MIT
//! Criterion bench: snapshot the synthetic 4-layer fixture into a fresh
//! on-disk store. Mirrors the §4.6 "snapshot p99 ≤ 500 ms" budget.

use criterion::{Criterion, criterion_group, criterion_main};
use pf_core::fixture::FixtureSpec;
use pf_core::store::PfStore;
use pf_microbench::make_snapshotter;
use tempfile::TempDir;

fn bench_snapshot_synthetic_4layer(c: &mut Criterion) {
    let spec = FixtureSpec::default();
    let snapper = make_snapshotter(spec);

    // One store reused across iterations so we measure the steady-state
    // (CoW dedup) snapshot path, not first-run zstd init.
    let dir = TempDir::new().unwrap();
    let store = PfStore::open(dir.path()).unwrap();
    // Warm-up.
    let _ = snapper.snapshot(&store, vec![]).unwrap();

    c.bench_function("snapshot_synthetic_4layer", |b| {
        b.iter(|| snapper.snapshot(&store, vec![]).unwrap());
    });
}

criterion_group!(benches, bench_snapshot_synthetic_4layer);
criterion_main!(benches);
