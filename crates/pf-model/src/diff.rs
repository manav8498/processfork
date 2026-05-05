// SPDX-License-Identifier: MIT
//! Typed weight-diff payloads for the four supported diff kinds.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Discriminator tag — useful for API consumers that just want the kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffKind {
    /// Low-rank adapters (LoRA).
    Lora,
    /// IA³ per-head scaling vectors.
    Ia3,
    /// Dense full-finetune delta.
    Full,
    /// In-place test-time training trace.
    InPlaceTtt,
}

/// One LoRA adapter for one matrix in one layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoraAdapter {
    /// Layer index.
    pub layer_id: u32,
    /// Which matrix this adapter targets (e.g. `"q_proj"`, `"v_proj"`).
    pub matrix: String,
    /// Adapter rank (= shared inner dim of A and B).
    pub rank: u32,
    /// Input dimension (= columns of A, rows of the original matrix).
    pub in_dim: u32,
    /// Output dimension (= rows of B, rows of the original matrix).
    pub out_dim: u32,
    /// `A` matrix, shape `[rank, in_dim]`, row-major.
    pub a: Vec<f32>,
    /// `B` matrix, shape `[out_dim, rank]`, row-major.
    pub b: Vec<f32>,
}

impl LoraAdapter {
    /// Verify the declared dimensions match the supplied vectors. Cheap;
    /// always called by `store_diff` before sealing.
    pub fn validate(&self) -> pf_core::Result<()> {
        let a_expected = (self.rank as usize) * (self.in_dim as usize);
        let b_expected = (self.out_dim as usize) * (self.rank as usize);
        if self.a.len() != a_expected {
            return Err(pf_core::Error::Integrity(format!(
                "LoraAdapter L{}/{}: a.len {} ≠ rank·in_dim {}",
                self.layer_id,
                self.matrix,
                self.a.len(),
                a_expected
            )));
        }
        if self.b.len() != b_expected {
            return Err(pf_core::Error::Integrity(format!(
                "LoraAdapter L{}/{}: b.len {} ≠ out_dim·rank {}",
                self.layer_id,
                self.matrix,
                self.b.len(),
                b_expected
            )));
        }
        Ok(())
    }
}

/// LoRA diff: a list of per-matrix adapters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoraDelta {
    /// Adapters sorted by `(layer_id, matrix)` for deterministic digests.
    pub adapters: Vec<LoraAdapter>,
}

impl LoraDelta {
    /// Sort adapters into canonical order so the diff's serialized digest
    /// is invariant w.r.t. caller iteration order.
    pub fn canonicalize(&mut self) {
        self.adapters
            .sort_by(|a, b| (a.layer_id, &a.matrix).cmp(&(b.layer_id, &b.matrix)));
    }
}

/// IA³ diff: per-layer per-matrix scaling vector.
///
/// Outer key: layer id, encoded as a base-10 string (e.g. `"0"`, `"31"`).
/// Stored as a `String` because JSON object keys are always strings; using
/// `String` here keeps the wire format trivially round-trippable.
/// Inner key: matrix name.
/// Value: scaling vector (length = head_dim).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IA3Delta {
    pub scaling: BTreeMap<String, BTreeMap<String, Vec<f32>>>,
}

/// Full-finetune diff: dense per-parameter delta tensors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FullDelta {
    /// Map from canonical parameter name to its dense delta.
    pub params: BTreeMap<String, Vec<f32>>,
}

/// One in-place TTT step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TttStep {
    /// Step counter (0-based).
    pub step_id: u32,
    /// Per-parameter delta applied at this step.
    pub deltas: BTreeMap<String, Vec<f32>>,
}

/// In-place TTT diff: an ordered trace of training steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InPlaceTttDelta {
    /// Steps in causal order.
    pub steps: Vec<TttStep>,
}

/// Top-level diff payload. Wire format `model.diff.v1`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ModelDiff {
    /// LoRA / low-rank adapters.
    #[serde(rename = "lora")]
    Lora(LoraDelta),
    /// IA³ scaling vectors.
    #[serde(rename = "ia3")]
    Ia3(IA3Delta),
    /// Dense full-finetune delta.
    #[serde(rename = "full")]
    Full(FullDelta),
    /// In-place test-time training trace.
    #[serde(rename = "in-place-ttt")]
    InPlaceTtt(InPlaceTttDelta),
}

impl ModelDiff {
    /// Discriminator.
    #[must_use]
    pub fn kind(&self) -> DiffKind {
        match self {
            Self::Lora(_) => DiffKind::Lora,
            Self::Ia3(_) => DiffKind::Ia3,
            Self::Full(_) => DiffKind::Full,
            Self::InPlaceTtt(_) => DiffKind::InPlaceTtt,
        }
    }

    /// Validate internal invariants and put the diff into canonical order.
    pub fn validate_and_canonicalize(&mut self) -> pf_core::Result<()> {
        match self {
            Self::Lora(d) => {
                d.canonicalize();
                for a in &d.adapters {
                    a.validate()?;
                }
            }
            Self::Ia3(_) | Self::Full(_) => {
                // BTreeMap is already canonical.
            }
            Self::InPlaceTtt(d) => {
                d.steps.sort_by_key(|s| s.step_id);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lora_validate_catches_dim_mismatch() {
        let bad = LoraAdapter {
            layer_id: 0,
            matrix: "q_proj".into(),
            rank: 4,
            in_dim: 8,
            out_dim: 8,
            a: vec![0.0; 4 * 8],
            b: vec![0.0; 5], // wrong
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn lora_canonicalize_orders_by_layer_then_matrix() {
        let mut d = LoraDelta {
            adapters: vec![
                LoraAdapter {
                    layer_id: 1,
                    matrix: "v_proj".into(),
                    rank: 2,
                    in_dim: 4,
                    out_dim: 4,
                    a: vec![0.0; 8],
                    b: vec![0.0; 8],
                },
                LoraAdapter {
                    layer_id: 0,
                    matrix: "v_proj".into(),
                    rank: 2,
                    in_dim: 4,
                    out_dim: 4,
                    a: vec![0.0; 8],
                    b: vec![0.0; 8],
                },
                LoraAdapter {
                    layer_id: 0,
                    matrix: "q_proj".into(),
                    rank: 2,
                    in_dim: 4,
                    out_dim: 4,
                    a: vec![0.0; 8],
                    b: vec![0.0; 8],
                },
            ],
        };
        d.canonicalize();
        assert_eq!(d.adapters[0].layer_id, 0);
        assert_eq!(d.adapters[0].matrix, "q_proj");
        assert_eq!(d.adapters[1].layer_id, 0);
        assert_eq!(d.adapters[1].matrix, "v_proj");
        assert_eq!(d.adapters[2].layer_id, 1);
    }

    #[test]
    fn kind_discriminator_matches_variant() {
        let lora = ModelDiff::Lora(LoraDelta { adapters: vec![] });
        assert_eq!(lora.kind(), DiffKind::Lora);
        let ia3 = ModelDiff::Ia3(IA3Delta {
            scaling: BTreeMap::new(),
        });
        assert_eq!(ia3.kind(), DiffKind::Ia3);
        let full = ModelDiff::Full(FullDelta {
            params: BTreeMap::new(),
        });
        assert_eq!(full.kind(), DiffKind::Full);
        let ttt = ModelDiff::InPlaceTtt(InPlaceTttDelta { steps: vec![] });
        assert_eq!(ttt.kind(), DiffKind::InPlaceTtt);
    }

    #[test]
    fn ttt_canonicalize_sorts_by_step_id() {
        let mut d = ModelDiff::InPlaceTtt(InPlaceTttDelta {
            steps: vec![
                TttStep {
                    step_id: 5,
                    deltas: BTreeMap::new(),
                },
                TttStep {
                    step_id: 1,
                    deltas: BTreeMap::new(),
                },
                TttStep {
                    step_id: 3,
                    deltas: BTreeMap::new(),
                },
            ],
        });
        d.validate_and_canonicalize().unwrap();
        if let ModelDiff::InPlaceTtt(t) = d {
            let ids: Vec<_> = t.steps.iter().map(|s| s.step_id).collect();
            assert_eq!(ids, vec![1, 3, 5]);
        } else {
            panic!("variant changed");
        }
    }
}
