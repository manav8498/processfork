// SPDX-License-Identifier: MIT
//! Top-level three-way merge engine.

use crate::MergeOutcome;
use crate::ancestor::{AncestorError, find_lca};
use crate::effects::merge_effects;
use crate::model::{ModelMergeOutcome, merge_model};
use crate::trace::{Summarizer, TraceMergeOutcome, merge_trace};
use crate::world::{WorldMergeOutcome, merge_world};

use chrono::Utc;
use pf_core::digest::Digest256;
use pf_core::manifest::{
    AgentInfo, CacheLayer, EffectsLayer, MEDIATYPE_V1, Manifest, ModelLayer, TraceLayer, WorldLayer,
};
use pf_core::store::PfStore;
use pf_effects::SideEffectClass;

/// Per-call merge knobs.
#[derive(Clone, Debug)]
pub struct MergeOptions {
    /// Mixing coefficient for the model layer's TIES merge.
    pub alpha: f32,
    /// DARE drop probability.
    pub dare_p: f64,
    /// PRNG seed (`PF_SEED` from the CLI).
    pub seed: u64,
    /// Side-effect classes the operator authorized for replay-with-new-key.
    /// Default empty: no irreversible re-issue.
    pub replay_with_new_key: Vec<SideEffectClass>,
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            alpha: 0.5,
            dare_p: 0.7,
            seed: 0,
            replay_with_new_key: Vec::new(),
        }
    }
}

/// What [`merge`] returns: the new manifest's CID + per-layer outcomes
/// + a list of conflicts the user needs to resolve.
#[derive(Debug)]
pub struct MergeReport {
    /// CID of the merged manifest.
    pub merged_cid: Digest256,
    /// CID of the discovered (or asserted) common ancestor.
    pub ancestor: Digest256,
    /// Trace-merge outcome.
    pub trace: TraceMergeOutcome,
    /// World-merge outcome.
    pub world: WorldMergeOutcome,
    /// Model-merge outcome.
    pub model: ModelMergeOutcome,
    /// CID of the merged effects blob (`effects.merged.v1`).
    pub effects: Digest256,
    /// Aggregate per-layer outcome for fast UX printing.
    pub per_layer: PerLayer,
    /// Top-level outcome.
    pub overall: MergeOutcome,
}

/// Aggregated per-layer outcome for the `pf merge` UX line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerLayer {
    pub trace: MergeOutcome,
    pub world: MergeOutcome,
    pub effects: MergeOutcome,
    pub model: MergeOutcome,
}

/// Three-way merge of B into A. Common ancestor X is auto-discovered via
/// [`find_lca`]; if you've already got it, pass it as `x_hint` to skip the
/// discovery walk.
///
/// On conflict the merged manifest IS still produced (with `<<<<<<<`-marker
/// blobs in the world layer); the caller checks `report.world.conflicts`
/// and either auto-resolves or surfaces them in `pf merge --tool`.
pub fn merge(
    store: &PfStore,
    a: &Digest256,
    b: &Digest256,
    summarizer: &dyn Summarizer,
    opts: &MergeOptions,
    x_hint: Option<Digest256>,
) -> Result<MergeReport, MergeError> {
    let ancestor = match x_hint {
        Some(x) => x,
        None => find_lca(store, a, b)?,
    };
    let a_m = store.get_manifest(a)?;
    let b_m = store.get_manifest(b)?;
    let x_m = store.get_manifest(&ancestor)?;

    let blobs = store.blobs();

    // Trace.
    let trace_out = merge_trace(
        blobs,
        &a_m.trace.messages,
        &b_m.trace.messages,
        &x_m.trace.messages,
        summarizer,
    )?;

    // World.
    let world_out = merge_world(blobs, &a_m.world.fs, &b_m.world.fs, &x_m.world.fs)?;

    // Effects.
    let effects_cid = merge_effects(
        blobs,
        &a_m.effects.ledger,
        &b_m.effects.ledger,
        &x_m.effects.ledger,
        opts.replay_with_new_key.clone(),
    )?;

    // Model.
    let model_out = merge_model(
        blobs,
        &a_m.model.diff,
        &b_m.model.diff,
        &x_m.model.diff,
        opts.alpha,
        opts.dare_p,
        opts.seed,
    )?;

    // Build new manifest. Cache layer is unchanged (per spec, A's KV-cache
    // pages are reused; only the appended trace is re-prefilled). Env and
    // procs come from A.
    let merged_manifest = Manifest {
        schema_version: 1,
        media_type: MEDIATYPE_V1.to_owned(),
        agent: AgentInfo {
            kind: a_m.agent.kind.clone(),
            version: a_m.agent.version.clone(),
            fingerprint: format!(
                "merge:{}+{}",
                short(&a_m.agent.fingerprint),
                short(&b_m.agent.fingerprint)
            ),
        },
        model: ModelLayer {
            base: a_m.model.base.clone(),
            diff: model_out.merged_diff.clone(),
        },
        cache: CacheLayer {
            layout: a_m.cache.layout.clone(),
            manifest: a_m.cache.manifest.clone(),
        },
        world: WorldLayer {
            fs: world_out.merged_fs.clone(),
            env: a_m.world.env.clone(),
            procs: a_m.world.procs.clone(),
        },
        effects: EffectsLayer {
            ledger: effects_cid.clone(),
        },
        trace: TraceLayer {
            messages: trace_out.merged_trace.clone(),
        },
        created_at: Utc::now(),
        parents: vec![a.clone(), b.clone()],
    };

    let merged_cid = store.put_manifest(&merged_manifest)?;

    let per_layer = PerLayer {
        trace: MergeOutcome::Clean,
        world: if world_out.conflicts.is_empty() {
            MergeOutcome::Clean
        } else {
            MergeOutcome::Conflicted
        },
        effects: MergeOutcome::Clean,
        model: if model_out.kind_mismatch {
            MergeOutcome::Conflicted
        } else {
            MergeOutcome::Clean
        },
    };
    let overall = if per_layer.world == MergeOutcome::Conflicted
        || per_layer.model == MergeOutcome::Conflicted
    {
        MergeOutcome::Conflicted
    } else {
        MergeOutcome::Clean
    };

    Ok(MergeReport {
        merged_cid,
        ancestor,
        trace: trace_out,
        world: world_out,
        model: model_out,
        effects: effects_cid,
        per_layer,
        overall,
    })
}

fn short(s: &str) -> &str {
    if s.len() > 8 { &s[..8] } else { s }
}

/// Engine-level error wrapper.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    /// Couldn't find a common ancestor.
    #[error("ancestor: {0}")]
    Ancestor(#[from] AncestorError),
    /// Inner layer error.
    #[error("layer: {0}")]
    Layer(#[from] pf_core::Error),
}

/// Convenience surface so callers that want one type don't have to match
/// `WorldConflict` directly. Re-export.
pub use crate::WorldConflict as _WorldConflict;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{StubSummarizer, TraceMessage};
    use chrono::Utc;
    use pf_core::cas::BlobStore;
    use pf_effects::{Ledger, SessionSecret, SideEffectClass};
    use pf_model::{LoraAdapter, LoraDelta, ModelDiff};
    use pf_world::{FsEntryKind, FsTree, FsTreeEntry};
    use tempfile::TempDir;

    fn write_trace(blobs: &dyn BlobStore, msgs: &[(&str, &str)]) -> Digest256 {
        let mut bytes = Vec::new();
        for (r, c) in msgs {
            let m = TraceMessage {
                role: (*r).into(),
                content: (*c).into(),
            };
            bytes.extend_from_slice(&serde_json::to_vec(&m).unwrap());
            bytes.push(b'\n');
        }
        blobs.put(&bytes).unwrap()
    }

    fn write_fs(blobs: &dyn BlobStore, files: &[(&str, &[u8])]) -> Digest256 {
        let entries: Vec<FsTreeEntry> = files
            .iter()
            .map(|(path, content)| {
                let blob = blobs.put(content).unwrap();
                FsTreeEntry {
                    path: (*path).into(),
                    mode: "0644".into(),
                    size: content.len() as u64,
                    kind: FsEntryKind::File,
                    blob: Some(blob),
                    link_target: None,
                }
            })
            .collect();
        let t = FsTree {
            kind: "fs.tree.v1".into(),
            entries,
        };
        blobs.put(&serde_json::to_vec(&t).unwrap()).unwrap()
    }

    fn write_ledger(blobs: &dyn BlobStore, n_irr: u32) -> Digest256 {
        let mut l = Ledger::new(SessionSecret::new(b"t".to_vec()));
        for i in 0..n_irr {
            l.append(
                Utc::now(),
                "send_email",
                Digest256::of(&[i as u8; 32]),
                format!("k{i}"),
                Digest256::of(&[i as u8 ^ 1; 32]),
                SideEffectClass::Irreversible,
            )
            .unwrap();
        }
        l.serialize(blobs).unwrap()
    }

    fn write_lora(blobs: &dyn BlobStore, val: f32) -> Digest256 {
        let d = ModelDiff::Lora(LoraDelta {
            adapters: vec![LoraAdapter {
                layer_id: 0,
                matrix: "q".into(),
                rank: 2,
                in_dim: 4,
                out_dim: 4,
                a: vec![val; 8],
                b: vec![val; 8],
            }],
        });
        pf_model::store_diff(blobs, d).unwrap()
    }

    fn put_full_manifest(
        store: &PfStore,
        parents: Vec<Digest256>,
        trace: Digest256,
        fs: Digest256,
        ledger: Digest256,
        model: Digest256,
        fingerprint: &str,
    ) -> Digest256 {
        let placeholder = store.blobs().put(b"_placeholder").unwrap();
        let m = Manifest {
            schema_version: 1,
            media_type: MEDIATYPE_V1.to_owned(),
            agent: AgentInfo {
                kind: "test".into(),
                version: "0".into(),
                fingerprint: fingerprint.into(),
            },
            model: ModelLayer {
                base: placeholder.clone(),
                diff: model,
            },
            cache: CacheLayer {
                layout: "paged-batchinvariant-v1".into(),
                manifest: placeholder.clone(),
            },
            world: WorldLayer {
                fs,
                env: placeholder.clone(),
                procs: placeholder,
            },
            effects: EffectsLayer { ledger },
            trace: TraceLayer { messages: trace },
            created_at: Utc::now(),
            parents,
        };
        store.put_manifest(&m).unwrap()
    }

    #[test]
    fn full_engine_round_trip_clean_merge() {
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let blobs = store.blobs();

        // Common ancestor X.
        let x_trace = write_trace(blobs, &[("user", "go")]);
        let x_fs = write_fs(blobs, &[("a.txt", b"orig"), ("b.txt", b"orig-b")]);
        let x_ledger = write_ledger(blobs, 0);
        let x_model = write_lora(blobs, 0.0);
        let x_cid = put_full_manifest(
            &store,
            vec![],
            x_trace.clone(),
            x_fs.clone(),
            x_ledger.clone(),
            x_model.clone(),
            "x",
        );

        // Branch A modifies a.txt.
        let a_trace = write_trace(blobs, &[("user", "go"), ("assistant", "main work")]);
        let a_fs = write_fs(blobs, &[("a.txt", b"A modified"), ("b.txt", b"orig-b")]);
        let a_ledger = write_ledger(blobs, 0);
        let a_model = write_lora(blobs, 1.0);
        let a_cid = put_full_manifest(
            &store,
            vec![x_cid.clone()],
            a_trace,
            a_fs,
            a_ledger,
            a_model,
            "a",
        );

        // Branch B modifies b.txt (no conflict with A) and sends an email.
        let b_trace = write_trace(blobs, &[("user", "go"), ("assistant", "sibling work")]);
        let b_fs = write_fs(blobs, &[("a.txt", b"orig"), ("b.txt", b"B modified")]);
        let b_ledger = write_ledger(blobs, 2);
        let b_model = write_lora(blobs, -1.0);
        let b_cid = put_full_manifest(
            &store,
            vec![x_cid.clone()],
            b_trace,
            b_fs,
            b_ledger,
            b_model,
            "b",
        );

        let report = merge(
            &store,
            &a_cid,
            &b_cid,
            &StubSummarizer,
            &MergeOptions::default(),
            None,
        )
        .unwrap();

        assert_eq!(report.ancestor, x_cid);
        assert_eq!(report.per_layer.world, MergeOutcome::Clean);
        assert_eq!(report.overall, MergeOutcome::Clean);
        assert!(report.world.conflicts.is_empty());

        // The merged manifest has both A and B as parents.
        let merged = store.get_manifest(&report.merged_cid).unwrap();
        assert_eq!(merged.parents, vec![a_cid, b_cid]);

        // Effects merged blob counts B's irreversibles correctly.
        let eff_bytes = blobs.get(&report.effects).unwrap();
        let eff: serde_json::Value = serde_json::from_slice(&eff_bytes).unwrap();
        assert_eq!(eff["summary"]["b_irreversible_cached"].as_u64(), Some(2));
    }

    #[test]
    fn engine_surfaces_world_conflicts() {
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let blobs = store.blobs();

        let x_trace = write_trace(blobs, &[("user", "go")]);
        let x_fs = write_fs(blobs, &[("a.txt", b"orig")]);
        let x_ledger = write_ledger(blobs, 0);
        let x_model = write_lora(blobs, 0.0);
        let x_cid = put_full_manifest(
            &store,
            vec![],
            x_trace.clone(),
            x_fs,
            x_ledger.clone(),
            x_model.clone(),
            "x",
        );

        let a_fs = write_fs(blobs, &[("a.txt", b"A change\n")]);
        let b_fs = write_fs(blobs, &[("a.txt", b"B change\n")]);

        let a_cid = put_full_manifest(
            &store,
            vec![x_cid.clone()],
            x_trace.clone(),
            a_fs,
            x_ledger.clone(),
            x_model.clone(),
            "a",
        );
        let b_cid = put_full_manifest(
            &store,
            vec![x_cid.clone()],
            x_trace,
            b_fs,
            x_ledger,
            x_model,
            "b",
        );

        let report = merge(
            &store,
            &a_cid,
            &b_cid,
            &StubSummarizer,
            &MergeOptions::default(),
            None,
        )
        .unwrap();

        assert_eq!(report.per_layer.world, MergeOutcome::Conflicted);
        assert_eq!(report.overall, MergeOutcome::Conflicted);
        assert_eq!(report.world.conflicts.len(), 1);
        assert_eq!(report.world.conflicts[0].path, "a.txt");
    }

    #[test]
    fn engine_uses_x_hint_when_provided() {
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let blobs = store.blobs();

        let x_trace = write_trace(blobs, &[]);
        let x_fs = write_fs(blobs, &[]);
        let x_ledger = write_ledger(blobs, 0);
        let x_model = write_lora(blobs, 0.0);
        let x_cid = put_full_manifest(
            &store,
            vec![],
            x_trace.clone(),
            x_fs.clone(),
            x_ledger.clone(),
            x_model.clone(),
            "x",
        );
        let a_cid = put_full_manifest(
            &store,
            vec![x_cid.clone()],
            x_trace.clone(),
            x_fs.clone(),
            x_ledger.clone(),
            x_model.clone(),
            "a",
        );
        let b_cid = put_full_manifest(
            &store,
            vec![x_cid.clone()],
            x_trace,
            x_fs,
            x_ledger,
            x_model,
            "b",
        );

        let report = merge(
            &store,
            &a_cid,
            &b_cid,
            &StubSummarizer,
            &MergeOptions::default(),
            Some(x_cid.clone()),
        )
        .unwrap();
        assert_eq!(report.ancestor, x_cid);
    }
}
