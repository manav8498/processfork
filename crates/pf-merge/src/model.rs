// SPDX-License-Identifier: MIT
//! Model-layer merge: variant-dispatch wrapper around `pf_model::ties_merge`
//! and `pf_model::dare`.

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_model::{
    DiffKind, FullDelta, IA3Delta, InPlaceTttDelta, LoraAdapter, LoraDelta, ModelDiff, TiesParams,
    dare, load_diff, store_diff, ties_merge,
};
use std::collections::BTreeMap;

/// What [`merge_model`] returns alongside the new diff digest.
#[derive(Clone, Debug)]
pub struct ModelMergeOutcome {
    /// Digest of the merged `model.diff.v1` blob.
    pub merged_diff: Digest256,
    /// Both inputs were trivial (empty / no overlap) — emitted A
    /// unchanged. The engine surfaces this as "no model merge needed."
    pub trivial: bool,
    /// Both inputs had non-empty deltas of the SAME kind — applied
    /// `TIES + DARE`. Engine surfaces this as a soft warning per
    /// `agent_docs/architecture.md` §4.4.
    pub applied_task_arithmetic: bool,
    /// Inputs had different kinds (e.g. A is LoRA, B is Full). v1 falls
    /// back to keeping A and surfacing this in the report.
    pub kind_mismatch: bool,
}

/// Three-way merge of A, B, X model diffs. `dare_p` defaults to 0.7,
/// `alpha` to 0.5 (per `agent_docs/architecture.md` §4.4).
pub fn merge_model(
    blobs: &dyn BlobStore,
    a: &Digest256,
    b: &Digest256,
    _x: &Digest256, // X is informational in v1; both branches diff from X
    alpha: f32,
    dare_p: f64,
    seed: u64,
) -> pf_core::Result<ModelMergeOutcome> {
    let a_diff = load_diff(blobs, a)?;
    let b_diff = load_diff(blobs, b)?;

    let ka = a_diff.kind();
    let kb = b_diff.kind();

    // Trivial branches.
    if is_trivial(&a_diff) && is_trivial(&b_diff) {
        return Ok(ModelMergeOutcome {
            merged_diff: store_diff(blobs, a_diff)?,
            trivial: true,
            applied_task_arithmetic: false,
            kind_mismatch: false,
        });
    }
    if is_trivial(&b_diff) {
        return Ok(ModelMergeOutcome {
            merged_diff: store_diff(blobs, a_diff)?,
            trivial: true,
            applied_task_arithmetic: false,
            kind_mismatch: false,
        });
    }
    if is_trivial(&a_diff) {
        return Ok(ModelMergeOutcome {
            merged_diff: store_diff(blobs, b_diff)?,
            trivial: true,
            applied_task_arithmetic: false,
            kind_mismatch: false,
        });
    }
    if ka != kb {
        // v1: keep A; record the mismatch.
        return Ok(ModelMergeOutcome {
            merged_diff: store_diff(blobs, a_diff)?,
            trivial: false,
            applied_task_arithmetic: false,
            kind_mismatch: true,
        });
    }

    let merged = match (a_diff, b_diff) {
        (ModelDiff::Lora(a_l), ModelDiff::Lora(b_l)) => {
            ModelDiff::Lora(merge_lora(&a_l, &b_l, alpha, dare_p, seed)?)
        }
        (ModelDiff::Full(a_f), ModelDiff::Full(b_f)) => {
            ModelDiff::Full(merge_full(&a_f, &b_f, alpha, dare_p, seed)?)
        }
        (ModelDiff::Ia3(a_i), ModelDiff::Ia3(b_i)) => {
            ModelDiff::Ia3(merge_ia3(&a_i, &b_i, alpha, dare_p, seed)?)
        }
        (ModelDiff::InPlaceTtt(a_t), ModelDiff::InPlaceTtt(b_t)) => {
            // Concat TTT step traces in causal order; not a TIES use case.
            let mut steps = a_t.steps;
            steps.extend(b_t.steps);
            steps.sort_by_key(|s| s.step_id);
            ModelDiff::InPlaceTtt(InPlaceTttDelta { steps })
        }
        // Unreachable because we guarded ka != kb above; but the compiler
        // doesn't know that.
        _ => return Err(pf_core::Error::Integrity("unreachable kind pair".into())),
    };

    let cid = store_diff(blobs, merged)?;
    Ok(ModelMergeOutcome {
        merged_diff: cid,
        trivial: false,
        applied_task_arithmetic: !matches!(ka, DiffKind::InPlaceTtt),
        kind_mismatch: false,
    })
}

fn is_trivial(d: &ModelDiff) -> bool {
    match d {
        ModelDiff::Lora(l) => l.adapters.is_empty(),
        ModelDiff::Full(f) => f.params.is_empty(),
        ModelDiff::Ia3(i) => i.scaling.is_empty(),
        ModelDiff::InPlaceTtt(t) => t.steps.is_empty(),
    }
}

// ----------- LoRA ----------------

fn merge_lora(
    a: &LoraDelta,
    b: &LoraDelta,
    alpha: f32,
    dare_p: f64,
    seed: u64,
) -> pf_core::Result<LoraDelta> {
    // Index by (layer_id, matrix). For shared keys, run TIES+DARE on each
    // of the A and B vectors. For exclusive keys, keep the side that has
    // them.
    let mut a_map: BTreeMap<(u32, &str), &LoraAdapter> = BTreeMap::new();
    for ad in &a.adapters {
        a_map.insert((ad.layer_id, ad.matrix.as_str()), ad);
    }
    let mut b_map: BTreeMap<(u32, &str), &LoraAdapter> = BTreeMap::new();
    for ad in &b.adapters {
        b_map.insert((ad.layer_id, ad.matrix.as_str()), ad);
    }
    let all_keys: std::collections::BTreeSet<_> =
        a_map.keys().chain(b_map.keys()).copied().collect();

    let mut adapters = Vec::new();
    for key in all_keys {
        let in_a = a_map.get(&key);
        let in_b = b_map.get(&key);
        match (in_a, in_b) {
            (Some(ad), None) | (None, Some(ad)) => adapters.push((**ad).clone()),
            (Some(ad_a), Some(ad_b)) => {
                if ad_a.rank != ad_b.rank
                    || ad_a.in_dim != ad_b.in_dim
                    || ad_a.out_dim != ad_b.out_dim
                {
                    return Err(pf_core::Error::Integrity(format!(
                        "LoRA shape mismatch on layer {} matrix {}: A=({}/{}/{}) B=({}/{}/{})",
                        ad_a.layer_id,
                        ad_a.matrix,
                        ad_a.rank,
                        ad_a.in_dim,
                        ad_a.out_dim,
                        ad_b.rank,
                        ad_b.in_dim,
                        ad_b.out_dim,
                    )));
                }
                let a_d = dare(&ad_a.a, dare_p, seed)?;
                let b_d = dare(&ad_b.a, dare_p, seed.wrapping_add(1))?;
                let merged_a = ties_merge(
                    &[&a_d, &b_d],
                    TiesParams {
                        keep_top: 0.2,
                        alpha,
                    },
                )?;
                let a_d2 = dare(&ad_a.b, dare_p, seed.wrapping_add(2))?;
                let b_d2 = dare(&ad_b.b, dare_p, seed.wrapping_add(3))?;
                let merged_b = ties_merge(
                    &[&a_d2, &b_d2],
                    TiesParams {
                        keep_top: 0.2,
                        alpha,
                    },
                )?;
                adapters.push(LoraAdapter {
                    layer_id: ad_a.layer_id,
                    matrix: ad_a.matrix.clone(),
                    rank: ad_a.rank,
                    in_dim: ad_a.in_dim,
                    out_dim: ad_a.out_dim,
                    a: merged_a,
                    b: merged_b,
                });
            }
            (None, None) => unreachable!(),
        }
    }
    Ok(LoraDelta { adapters })
}

// ----------- Full ----------------

fn merge_full(
    a: &FullDelta,
    b: &FullDelta,
    alpha: f32,
    dare_p: f64,
    seed: u64,
) -> pf_core::Result<FullDelta> {
    let all: std::collections::BTreeSet<_> = a.params.keys().chain(b.params.keys()).collect();
    let mut params = BTreeMap::new();
    for (i, k) in all.into_iter().enumerate() {
        let in_a = a.params.get(k);
        let in_b = b.params.get(k);
        let merged_param = match (in_a, in_b) {
            (Some(v), None) | (None, Some(v)) => v.clone(),
            (Some(va), Some(vb)) => {
                if va.len() != vb.len() {
                    return Err(pf_core::Error::Integrity(format!(
                        "Full param {k}: A.len {} ≠ B.len {}",
                        va.len(),
                        vb.len()
                    )));
                }
                let a_d = dare(va, dare_p, seed.wrapping_add(i as u64))?;
                let b_d = dare(vb, dare_p, seed.wrapping_add(i as u64 ^ 0xFF))?;
                ties_merge(
                    &[&a_d, &b_d],
                    TiesParams {
                        keep_top: 0.2,
                        alpha,
                    },
                )?
            }
            (None, None) => unreachable!(),
        };
        params.insert(k.clone(), merged_param);
    }
    Ok(FullDelta { params })
}

// ----------- IA³ ----------------

fn merge_ia3(
    a: &IA3Delta,
    b: &IA3Delta,
    alpha: f32,
    dare_p: f64,
    seed: u64,
) -> pf_core::Result<IA3Delta> {
    let mut out: BTreeMap<String, BTreeMap<String, Vec<f32>>> = BTreeMap::new();
    let layers: std::collections::BTreeSet<_> = a.scaling.keys().chain(b.scaling.keys()).collect();
    let mut counter: u64 = 0;
    for layer in layers {
        let a_inner = a.scaling.get(layer);
        let b_inner = b.scaling.get(layer);
        let mut layer_out = BTreeMap::new();
        let matrices: std::collections::BTreeSet<_> = a_inner
            .into_iter()
            .flat_map(|m| m.keys())
            .chain(b_inner.into_iter().flat_map(|m| m.keys()))
            .collect();
        for matrix in matrices {
            let va = a_inner.and_then(|m| m.get(matrix));
            let vb = b_inner.and_then(|m| m.get(matrix));
            let merged_vec = match (va, vb) {
                (Some(v), None) | (None, Some(v)) => v.clone(),
                (Some(va), Some(vb)) => {
                    if va.len() != vb.len() {
                        return Err(pf_core::Error::Integrity(format!(
                            "IA3 layer {layer} matrix {matrix}: len mismatch {} ≠ {}",
                            va.len(),
                            vb.len(),
                        )));
                    }
                    let a_d = dare(va, dare_p, seed.wrapping_add(counter))?;
                    counter = counter.wrapping_add(1);
                    let b_d = dare(vb, dare_p, seed.wrapping_add(counter))?;
                    counter = counter.wrapping_add(1);
                    ties_merge(
                        &[&a_d, &b_d],
                        TiesParams {
                            keep_top: 0.2,
                            alpha,
                        },
                    )?
                }
                (None, None) => unreachable!(),
            };
            layer_out.insert(matrix.clone(), merged_vec);
        }
        out.insert(layer.clone(), layer_out);
    }
    Ok(IA3Delta { scaling: out })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::cas::MemBlobStore;

    fn put_lora(blobs: &dyn BlobStore, layer: u32, val: f32) -> Digest256 {
        let d = ModelDiff::Lora(LoraDelta {
            adapters: vec![LoraAdapter {
                layer_id: layer,
                matrix: "q".into(),
                rank: 2,
                in_dim: 4,
                out_dim: 4,
                a: vec![val; 8],
                b: vec![val; 8],
            }],
        });
        store_diff(blobs, d).unwrap()
    }

    fn put_empty_lora(blobs: &dyn BlobStore) -> Digest256 {
        store_diff(blobs, ModelDiff::Lora(LoraDelta { adapters: vec![] })).unwrap()
    }

    #[test]
    fn trivial_when_both_empty() {
        let blobs = MemBlobStore::new();
        let empty = put_empty_lora(&blobs);
        let r = merge_model(&blobs, &empty, &empty, &empty, 0.5, 0.7, 0).unwrap();
        assert!(r.trivial);
        assert!(!r.applied_task_arithmetic);
    }

    #[test]
    fn trivial_when_one_empty() {
        let blobs = MemBlobStore::new();
        let empty = put_empty_lora(&blobs);
        let a = put_lora(&blobs, 0, 1.0);
        let r = merge_model(&blobs, &a, &empty, &empty, 0.5, 0.7, 0).unwrap();
        assert!(r.trivial);
        let merged = load_diff(&blobs, &r.merged_diff).unwrap();
        if let ModelDiff::Lora(d) = merged {
            assert_eq!(d.adapters.len(), 1);
        } else {
            panic!("variant changed");
        }
    }

    #[test]
    fn applies_task_arithmetic_on_overlap() {
        let blobs = MemBlobStore::new();
        let a = put_lora(&blobs, 0, 1.0);
        let b = put_lora(&blobs, 0, -1.0);
        let x = put_empty_lora(&blobs);
        let r = merge_model(&blobs, &a, &b, &x, 0.5, 0.7, 7).unwrap();
        assert!(!r.trivial);
        assert!(r.applied_task_arithmetic);
        // Output should be a Lora with one adapter, finite values.
        let merged = load_diff(&blobs, &r.merged_diff).unwrap();
        if let ModelDiff::Lora(d) = merged {
            assert_eq!(d.adapters.len(), 1);
            for v in d.adapters[0].a.iter().chain(d.adapters[0].b.iter()) {
                assert!(v.is_finite());
            }
        } else {
            panic!("variant changed");
        }
    }

    #[test]
    fn kind_mismatch_keeps_a_and_flags() {
        let blobs = MemBlobStore::new();
        let a = put_lora(&blobs, 0, 1.0);
        let mut full_params = BTreeMap::new();
        full_params.insert("p".into(), vec![1.0_f32; 4]);
        let b = store_diff(
            &blobs,
            ModelDiff::Full(FullDelta {
                params: full_params,
            }),
        )
        .unwrap();
        let x = put_empty_lora(&blobs);
        let r = merge_model(&blobs, &a, &b, &x, 0.5, 0.7, 0).unwrap();
        assert!(r.kind_mismatch);
        let merged = load_diff(&blobs, &r.merged_diff).unwrap();
        assert_eq!(merged.kind(), DiffKind::Lora);
    }

    #[test]
    fn lora_shape_mismatch_errors() {
        let blobs = MemBlobStore::new();
        let a = put_lora(&blobs, 0, 1.0); // rank 2, in 4, out 4
        let b_big = ModelDiff::Lora(LoraDelta {
            adapters: vec![LoraAdapter {
                layer_id: 0,
                matrix: "q".into(),
                rank: 4, // diverges
                in_dim: 4,
                out_dim: 4,
                a: vec![0.0; 16],
                b: vec![0.0; 16],
            }],
        });
        let b = store_diff(&blobs, b_big).unwrap();
        let x = put_empty_lora(&blobs);
        let err = merge_model(&blobs, &a, &b, &x, 0.5, 0.7, 0).unwrap_err();
        assert!(matches!(err, pf_core::Error::Integrity(_)));
    }
}
