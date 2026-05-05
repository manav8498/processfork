// SPDX-License-Identifier: MIT
//! Phase-6 acceptance: build a fork-pair from a real Phase-1 fixture
//! snapshot, merge them through the engine, assert the merged manifest
//! is reachable and consistent.
//!
//! Uses the synthetic 4-layer fixture from `pf_core::fixture` so the
//! merge engine is exercised against the same workload Phases 1–5 used
//! end-to-end.

use std::sync::Arc;

use pf_core::fixture::{
    FixtureCacheCapture, FixtureEffectsCapture, FixtureModelCapture, FixtureSpec,
    FixtureTraceCapture, FixtureWorldCapture,
};
use pf_core::manifest::AgentInfo;
use pf_core::snapshot::Snapshotter;
use pf_core::store::PfStore;
use pf_merge::{MergeOptions, MergeOutcome, StubSummarizer, merge};
use tempfile::TempDir;

fn snapshotter(spec: FixtureSpec) -> Snapshotter {
    let agent = AgentInfo {
        kind: "test".into(),
        version: "0".into(),
        fingerprint: format!("seed-{}", spec.seed),
    };
    Snapshotter::new(
        agent,
        Arc::new(FixtureModelCapture(spec.clone())),
        Arc::new(FixtureCacheCapture(spec.clone())),
        Arc::new(FixtureWorldCapture(spec.clone())),
        Arc::new(FixtureEffectsCapture(spec.clone())),
        Arc::new(FixtureTraceCapture(spec)),
    )
}

#[test]
fn merge_two_synthetic_forks_end_to_end() {
    let dir = TempDir::new().unwrap();
    let store = PfStore::open(dir.path()).unwrap();

    // Common ancestor X: seed 0.
    let x_cid = snapshotter(FixtureSpec {
        seed: 0,
        ..FixtureSpec::default()
    })
    .snapshot(&store, vec![])
    .unwrap();

    // Branch A: seed 1, parent X.
    let a_cid = snapshotter(FixtureSpec {
        seed: 1,
        ..FixtureSpec::default()
    })
    .snapshot(&store, vec![x_cid.clone()])
    .unwrap();

    // Branch B: seed 2, parent X.
    let b_cid = snapshotter(FixtureSpec {
        seed: 2,
        ..FixtureSpec::default()
    })
    .snapshot(&store, vec![x_cid.clone()])
    .unwrap();

    // Merge.
    let report = merge(
        &store,
        &a_cid,
        &b_cid,
        &StubSummarizer,
        &MergeOptions::default(),
        None,
    )
    .unwrap();

    // LCA discovery hit X.
    assert_eq!(report.ancestor, x_cid);

    // The fixture's per-seed payloads diverge so the world layer will
    // typically conflict (the synthetic FS files use the seed in their
    // bytes). We just assert the engine reached a verdict and the
    // merged manifest is loadable.
    assert!(matches!(
        report.overall,
        MergeOutcome::Clean | MergeOutcome::Conflicted
    ));
    let merged = store.get_manifest(&report.merged_cid).unwrap();
    assert_eq!(merged.parents, vec![a_cid, b_cid]);
    assert_eq!(merged.schema_version, 1);
    assert_eq!(merged.cache.layout, "paged-batchinvariant-v1");
    // Trace blob exists and decodes.
    let trace_bytes = store.blobs().get(&merged.trace.messages).unwrap();
    assert!(!trace_bytes.is_empty());
}

#[test]
fn merge_lca_chain_is_walked() {
    // X → A → A2 ;  X → B → B2 . Merging A2 with B2 should still find X.
    let dir = TempDir::new().unwrap();
    let store = PfStore::open(dir.path()).unwrap();

    let x = snapshotter(FixtureSpec::default())
        .snapshot(&store, vec![])
        .unwrap();
    let a1 = snapshotter(FixtureSpec {
        seed: 1,
        ..FixtureSpec::default()
    })
    .snapshot(&store, vec![x.clone()])
    .unwrap();
    let a2 = snapshotter(FixtureSpec {
        seed: 2,
        ..FixtureSpec::default()
    })
    .snapshot(&store, vec![a1.clone()])
    .unwrap();
    let b1 = snapshotter(FixtureSpec {
        seed: 3,
        ..FixtureSpec::default()
    })
    .snapshot(&store, vec![x.clone()])
    .unwrap();
    let b2 = snapshotter(FixtureSpec {
        seed: 4,
        ..FixtureSpec::default()
    })
    .snapshot(&store, vec![b1.clone()])
    .unwrap();

    let report = merge(
        &store,
        &a2,
        &b2,
        &StubSummarizer,
        &MergeOptions::default(),
        None,
    )
    .unwrap();
    assert_eq!(report.ancestor, x);
}

#[test]
fn idempotent_merge_uses_x_hint_path() {
    // Self-merge: A ⨝ A. Engine should detect ancestor = A and emit a
    // clean merged manifest with both parents = A.
    let dir = TempDir::new().unwrap();
    let store = PfStore::open(dir.path()).unwrap();
    let a = snapshotter(FixtureSpec::default())
        .snapshot(&store, vec![])
        .unwrap();
    let report = merge(
        &store,
        &a,
        &a,
        &StubSummarizer,
        &MergeOptions::default(),
        None,
    )
    .unwrap();
    assert_eq!(report.ancestor, a);
    assert_eq!(report.overall, MergeOutcome::Clean);
}
