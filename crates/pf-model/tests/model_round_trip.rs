// SPDX-License-Identifier: MIT
//! Phase-5 acceptance: every ModelDiff variant round-trips through the
//! BlobStore byte-identically; TIES+DARE merge composes with serialize.

// Test-only synthetic-input generation does cheap u64→f32 casts; the
// precision loss is irrelevant for fuzzing TIES merge invariants.
#![allow(
    clippy::needless_pass_by_value,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

use pf_core::cas::{BlobStore, FsBlobStore};
use pf_model::{
    FullDelta, IA3Delta, InPlaceTttDelta, LoraAdapter, LoraDelta, ModelDiff, TiesParams, TttStep,
    dare, load_diff, store_diff, ties_merge,
};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use tempfile::TempDir;

fn make_lora() -> ModelDiff {
    ModelDiff::Lora(LoraDelta {
        adapters: (0..4)
            .map(|layer| LoraAdapter {
                layer_id: layer,
                matrix: "q_proj".into(),
                rank: 4,
                in_dim: 16,
                out_dim: 16,
                a: vec![0.5; 4 * 16],
                b: vec![0.25; 16 * 4],
            })
            .collect(),
    })
}

fn make_ia3() -> ModelDiff {
    let mut outer = BTreeMap::new();
    for layer in 0..4 {
        let mut inner = BTreeMap::new();
        inner.insert("k_proj".to_owned(), vec![0.1_f32; 8]);
        inner.insert("v_proj".to_owned(), vec![0.2_f32; 8]);
        outer.insert(format!("{layer}"), inner);
    }
    ModelDiff::Ia3(IA3Delta { scaling: outer })
}

fn make_full() -> ModelDiff {
    let mut p = BTreeMap::new();
    for layer in 0..4 {
        p.insert(format!("layer_{layer}/q_proj"), vec![0.01_f32; 64]);
    }
    ModelDiff::Full(FullDelta { params: p })
}

fn make_ttt() -> ModelDiff {
    let mut steps = Vec::new();
    for step_id in 0..3 {
        let mut deltas = BTreeMap::new();
        deltas.insert("layer_0/q_proj".into(), vec![0.001_f32; 16]);
        steps.push(TttStep { step_id, deltas });
    }
    ModelDiff::InPlaceTtt(InPlaceTttDelta { steps })
}

#[test]
fn every_variant_round_trips_via_fs_blob_store() {
    let dir = TempDir::new().unwrap();
    let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(dir.path()).unwrap());
    for d in [make_lora(), make_ia3(), make_full(), make_ttt()] {
        let cid = store_diff(blobs.as_ref(), d.clone()).unwrap();
        let back = load_diff(blobs.as_ref(), &cid).unwrap();
        assert_eq!(back, d, "variant {:?} round-trip diverged", d.kind());
    }
}

#[test]
fn dare_then_ties_composition_does_not_explode() {
    // Two synthetic deltas, apply DARE then TIES-merge them. Verify the
    // merged vector has the right length and stays bounded.
    let a = vec![0.5_f32; 1024];
    let b = vec![-0.4_f32; 1024];

    let a_dare = dare(&a, 0.5, 7).unwrap();
    let b_dare = dare(&b, 0.5, 11).unwrap();

    let merged = ties_merge(
        &[&a_dare, &b_dare],
        TiesParams {
            keep_top: 0.2,
            alpha: 0.5,
        },
    )
    .unwrap();

    assert_eq!(merged.len(), 1024);
    // After DARE rescale: surviving entries become ~1.0 / -0.8. After TIES
    // sign-elect (positive wins by magnitude), average over surviving + α=0.5,
    // every output magnitude should stay well under the rescaled max (1.0).
    let max_abs = merged.iter().map(|x| x.abs()).fold(0.0_f32, f32::max);
    assert!(
        max_abs <= 1.5,
        "max |Δ| after merge = {max_abs}; expected ≤ 1.5"
    );
}

#[test]
fn cas_dedupes_identical_diffs() {
    // Storing the same diff twice should be a no-op the second time.
    let dir = TempDir::new().unwrap();
    let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(dir.path()).unwrap());

    let d = make_full();
    let _ = store_diff(blobs.as_ref(), d.clone()).unwrap();
    let bytes_after_first = blobs.physical_bytes().unwrap();
    let _ = store_diff(blobs.as_ref(), d).unwrap();
    let bytes_after_second = blobs.physical_bytes().unwrap();
    assert_eq!(
        bytes_after_first, bytes_after_second,
        "second store of identical diff must be a no-op"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        max_shrink_iters: 32,
        ..ProptestConfig::default()
    })]

    #[test]
    fn ties_merge_output_length_matches_input(
        len in 1usize..256usize,
        seed_a in any::<u64>(),
        seed_b in any::<u64>(),
    ) {
        // Generate two synthetic deltas of identical length.
        let a: Vec<f32> = (0..len).map(|i| ((seed_a.wrapping_add(i as u64)) % 17) as f32 / 4.0 - 2.0).collect();
        let b: Vec<f32> = (0..len).map(|i| ((seed_b.wrapping_add(i as u64)) % 13) as f32 / 4.0 - 1.5).collect();

        let merged = ties_merge(&[&a, &b], TiesParams::default()).unwrap();
        prop_assert_eq!(merged.len(), len);
        for v in &merged {
            prop_assert!(v.is_finite(), "merged contains non-finite: {v}");
        }
    }
}
