// SPDX-License-Identifier: MIT
//! Round-trip every [`ModelDiff`] variant through a [`pf_core::cas::BlobStore`].
//!
//! Wire format `model.diff.v1`: a single JSON blob containing a tagged
//! [`ModelDiff`]. We rely on `serde_tagged` semantics from the enum's
//! `#[serde(tag = "kind")]` for the discriminator. Validation +
//! canonicalization happen on store; layout-version check happens on load.

use crate::diff::ModelDiff;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use serde::{Deserialize, Serialize};

const LAYOUT: &str = "model.diff.v1";

#[derive(Serialize, Deserialize)]
struct Envelope {
    layout: String,
    diff: ModelDiff,
}

/// Validate, canonicalize, and persist a [`ModelDiff`] into `blobs`. Returns
/// the digest of the resulting blob.
pub fn store_diff(blobs: &dyn BlobStore, mut diff: ModelDiff) -> pf_core::Result<Digest256> {
    diff.validate_and_canonicalize()?;
    let env = Envelope {
        layout: LAYOUT.into(),
        diff,
    };
    blobs.put(&serde_json::to_vec(&env)?)
}

/// Load a [`ModelDiff`] previously written by [`store_diff`].
pub fn load_diff(blobs: &dyn BlobStore, digest: &Digest256) -> pf_core::Result<ModelDiff> {
    let bytes = blobs.get(digest)?;
    let env: Envelope = serde_json::from_slice(&bytes)?;
    if env.layout != LAYOUT {
        return Err(pf_core::Error::Integrity(format!(
            "expected layout {LAYOUT}, got {}",
            env.layout
        )));
    }
    Ok(env.diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FullDelta, IA3Delta, InPlaceTttDelta, LoraAdapter, LoraDelta, TttStep};
    use pf_core::cas::MemBlobStore;
    use std::collections::BTreeMap;

    #[test]
    fn lora_round_trip() {
        let blobs = MemBlobStore::new();
        let d = ModelDiff::Lora(LoraDelta {
            adapters: vec![LoraAdapter {
                layer_id: 0,
                matrix: "q_proj".into(),
                rank: 2,
                in_dim: 4,
                out_dim: 4,
                a: vec![1.0; 8],
                b: vec![2.0; 8],
            }],
        });
        let cid = store_diff(&blobs, d.clone()).unwrap();
        let back = load_diff(&blobs, &cid).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn ia3_round_trip() {
        let blobs = MemBlobStore::new();
        let mut s = BTreeMap::new();
        let mut inner = BTreeMap::new();
        inner.insert("k_proj".to_owned(), vec![0.5_f32, 1.5_f32]);
        s.insert("0".to_owned(), inner);
        let d = ModelDiff::Ia3(IA3Delta { scaling: s });
        let cid = store_diff(&blobs, d.clone()).unwrap();
        let back = load_diff(&blobs, &cid).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn full_round_trip() {
        let blobs = MemBlobStore::new();
        let mut p = BTreeMap::new();
        p.insert("layer_0/q_proj".to_owned(), vec![0.1_f32, 0.2_f32, 0.3_f32]);
        let d = ModelDiff::Full(FullDelta { params: p });
        let cid = store_diff(&blobs, d.clone()).unwrap();
        let back = load_diff(&blobs, &cid).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn ttt_round_trip_in_canonical_order() {
        let blobs = MemBlobStore::new();
        let mut step_a = TttStep {
            step_id: 2,
            deltas: BTreeMap::new(),
        };
        step_a.deltas.insert("x".into(), vec![1.0]);
        let step_b = TttStep {
            step_id: 1,
            deltas: BTreeMap::new(),
        };
        let d = ModelDiff::InPlaceTtt(InPlaceTttDelta {
            steps: vec![step_a, step_b],
        });
        let cid = store_diff(&blobs, d).unwrap();
        let back = load_diff(&blobs, &cid).unwrap();
        if let ModelDiff::InPlaceTtt(t) = back {
            assert_eq!(t.steps[0].step_id, 1, "canonicalize sorted by step_id");
            assert_eq!(t.steps[1].step_id, 2);
        } else {
            panic!("variant changed");
        }
    }

    #[test]
    fn rejects_wrong_layout_on_load() {
        let blobs = MemBlobStore::new();
        let bogus = serde_json::json!({
            "layout": "model.diff.v9",
            "diff": { "kind": "lora", "adapters": [] }
        });
        let cid = blobs.put(&serde_json::to_vec(&bogus).unwrap()).unwrap();
        let err = load_diff(&blobs, &cid).unwrap_err();
        assert!(matches!(err, pf_core::Error::Integrity(_)));
    }

    #[test]
    fn rejects_lora_with_wrong_dims_on_store() {
        let blobs = MemBlobStore::new();
        let d = ModelDiff::Lora(LoraDelta {
            adapters: vec![LoraAdapter {
                layer_id: 0,
                matrix: "q".into(),
                rank: 2,
                in_dim: 4,
                out_dim: 4,
                a: vec![0.0; 8],
                b: vec![0.0; 7], // wrong
            }],
        });
        assert!(store_diff(&blobs, d).is_err());
    }
}
