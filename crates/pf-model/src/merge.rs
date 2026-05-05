// SPDX-License-Identifier: MIT
//! TIES + DARE task-arithmetic merge primitives.
//!
//! References:
//! - DARE: "Language Models are Super Mario" (Yu et al., 2023).
//! - TIES: "TIES-Merging: Resolving Interference When Merging Models"
//!   (Yadav et al., 2023).
//!
//! Both algorithms operate on flat parameter-delta vectors and are
//! cheap to test on small synthetic tensors. The mergekit-equivalence
//! test against an external Python reference is GPU-gated (it needs
//! Llama-3-8B base weights).

/// DARE: Drop A Random Element.
///
/// For each entry of `delta`, with probability `p`, set it to zero;
/// otherwise rescale by `1 / (1 - p)`. The expectation of each entry is
/// preserved; the variance increases. Reduces interference at merge time.
///
/// `seed` makes the output reproducible. The PRNG is a deterministic
/// SplitMix64 streamed from the seed; **not** cryptographic, fine for
/// merge.
///
/// `p` must be in `(0.0, 1.0)`; out-of-range returns
/// [`pf_core::Error::Integrity`].
pub fn dare(delta: &[f32], p: f64, seed: u64) -> pf_core::Result<Vec<f32>> {
    if !(p > 0.0 && p < 1.0) {
        return Err(pf_core::Error::Integrity(format!(
            "dare: p must be in (0,1), got {p}"
        )));
    }
    let scale = 1.0 / (1.0 - p);
    let mut out = Vec::with_capacity(delta.len());
    let mut rng = SplitMix64::new(seed);
    for &x in delta {
        // Sample u in [0, 1) — top 53 bits of u64 → f64. The `u64 → f64`
        // cast is exact for the top-53-bit value (≤ 2^53); the divisor is
        // a const power of two with exact f64 representation. Allowed.
        #[allow(clippy::cast_precision_loss)]
        let u = (rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64;
        if u < p {
            out.push(0.0);
        } else {
            // Cast loss between f64 scale and f32 element is acceptable —
            // we round-trip through f64 only for the rescale arithmetic.
            #[allow(clippy::cast_possible_truncation)]
            out.push((f64::from(x) * scale) as f32);
        }
    }
    Ok(out)
}

/// Per-call TIES tuning knobs.
#[derive(Clone, Copy, Debug)]
pub struct TiesParams {
    /// Magnitude trim threshold: parameters whose `|Δ|` is below the
    /// `keep_top` quantile are zeroed before sign-election. `0.2` keeps the
    /// top 80 % by magnitude (the default in the TIES paper).
    pub keep_top: f64,
    /// Mixing coefficient applied to the elected merge. Default `0.5`
    /// (50/50 — see `agent_docs/architecture.md` §4.4).
    pub alpha: f32,
}

impl Default for TiesParams {
    fn default() -> Self {
        Self {
            keep_top: 0.2,
            alpha: 0.5,
        }
    }
}

/// TIES merge of N flat delta vectors of identical length.
///
/// Steps (per the TIES paper):
/// 1. **Trim**: per-vector, zero out the bottom `keep_top` quantile by
///    magnitude.
/// 2. **Elect sign**: per-position, the sign with the larger summed
///    magnitude wins.
/// 3. **Disjoint merge**: per-position, average the values that match the
///    elected sign (zeros from trimming or wrong-sign entries excluded).
/// 4. **Scale**: multiply by `alpha`.
///
/// Returns a single merged delta of the same length.
pub fn ties_merge(deltas: &[&[f32]], params: TiesParams) -> pf_core::Result<Vec<f32>> {
    if deltas.is_empty() {
        return Err(pf_core::Error::Integrity("ties_merge: no deltas".into()));
    }
    let len = deltas[0].len();
    for (i, d) in deltas.iter().enumerate() {
        if d.len() != len {
            return Err(pf_core::Error::Integrity(format!(
                "ties_merge: delta {i} has len {}, expected {len}",
                d.len()
            )));
        }
    }
    if !(params.keep_top >= 0.0 && params.keep_top <= 1.0) {
        return Err(pf_core::Error::Integrity(format!(
            "ties_merge: keep_top must be in [0,1], got {}",
            params.keep_top
        )));
    }

    // Step 1: trim each delta in-place into a Vec<f32>.
    let trimmed: Vec<Vec<f32>> = deltas
        .iter()
        .map(|d| trim_bottom(d, params.keep_top))
        .collect();

    // Step 2 + 3: per-position sign-elect and disjoint merge.
    let mut out = Vec::with_capacity(len);
    for ix in 0..len {
        let (mut pos_mag, mut neg_mag) = (0.0_f64, 0.0_f64);
        for d in &trimmed {
            let v = d[ix];
            if v > 0.0 {
                pos_mag += f64::from(v);
            } else if v < 0.0 {
                neg_mag += f64::from(-v);
            }
        }
        let elected_sign = if pos_mag >= neg_mag {
            1.0_f32
        } else {
            -1.0_f32
        };
        let mut sum = 0.0_f64;
        let mut n = 0_u32;
        for d in &trimmed {
            let v = d[ix];
            if (v > 0.0 && elected_sign > 0.0) || (v < 0.0 && elected_sign < 0.0) {
                sum += f64::from(v);
                n += 1;
            }
        }
        let merged = if n == 0 { 0.0 } else { sum / f64::from(n) };
        #[allow(clippy::cast_possible_truncation)]
        out.push((merged * f64::from(params.alpha)) as f32);
    }
    Ok(out)
}

/// Zero out the bottom `quantile` fraction by magnitude. `quantile = 0`
/// keeps everything; `quantile = 1` zeros everything.
fn trim_bottom(delta: &[f32], quantile: f64) -> Vec<f32> {
    if quantile <= 0.0 {
        return delta.to_vec();
    }
    if quantile >= 1.0 {
        return vec![0.0; delta.len()];
    }
    let mut mags: Vec<f32> = delta.iter().map(|x| x.abs()).collect();
    mags.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Cast: idx-arith is bounded by len, and len fits in usize. Precision
    // loss past 2^52 is acceptable — we only need a quantile *index*, not
    // a precise count, and operational delta lengths stay well under 2^52.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let cut_ix = (quantile * delta.len() as f64) as usize;
    let cut_ix = cut_ix.min(delta.len().saturating_sub(1));
    let threshold = mags[cut_ix];
    // Strict `<` so trimming "the bottom 50%" with len=4 actually drops 2,
    // keeps 2 (rather than dropping a third element when its magnitude
    // equals the threshold).
    delta
        .iter()
        .map(|&x| if x.abs() < threshold { 0.0 } else { x })
        .collect()
}

/// SplitMix64 PRNG — same family used in `pf-core::fixture` and
/// `pf-cache::pager`. Cheap, deterministic, no crate dep.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dare_is_deterministic_for_same_seed() {
        let delta = vec![1.0_f32; 256];
        let a = dare(&delta, 0.7, 42).unwrap();
        let b = dare(&delta, 0.7, 42).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn dare_drops_roughly_p_fraction() {
        let delta = vec![1.0_f32; 10_000];
        let out = dare(&delta, 0.7, 1).unwrap();
        let zeros = out.iter().filter(|&&x| x == 0.0).count();
        // p=0.7 → ~70 % zeros. Generous slack: 60-80 %.
        assert!(
            (6_000..=8_000).contains(&zeros),
            "got {zeros} zeros out of 10000"
        );
    }

    #[test]
    fn dare_rescales_survivors_by_inverse() {
        // With p=0.5, surviving entries should be 2× the input.
        let delta = vec![3.0_f32; 1000];
        let out = dare(&delta, 0.5, 0).unwrap();
        for &v in &out {
            assert!(v == 0.0 || (v - 6.0).abs() < 1e-3, "got {v}");
        }
    }

    #[test]
    fn dare_rejects_bad_p() {
        let delta = vec![1.0; 1];
        assert!(dare(&delta, 0.0, 0).is_err());
        assert!(dare(&delta, 1.0, 0).is_err());
        assert!(dare(&delta, -0.5, 0).is_err());
    }

    #[test]
    fn ties_merge_two_aligned_deltas_averages_them() {
        let a = vec![1.0_f32, 2.0, 3.0, 4.0];
        let b = vec![1.0_f32, 2.0, 3.0, 4.0];
        // No trimming, alpha=1: result should equal the input.
        let out = ties_merge(
            &[&a, &b],
            TiesParams {
                keep_top: 0.0,
                alpha: 1.0,
            },
        )
        .unwrap();
        for (i, v) in out.iter().enumerate() {
            assert!((v - a[i]).abs() < 1e-5, "ix {i}: {v} ≠ {}", a[i]);
        }
    }

    #[test]
    fn ties_merge_resolves_sign_conflict_by_majority_magnitude() {
        // Two vectors, opposite signs at index 0. Larger magnitude wins.
        let a = vec![3.0_f32];
        let b = vec![-1.0_f32];
        let out = ties_merge(
            &[&a, &b],
            TiesParams {
                keep_top: 0.0,
                alpha: 1.0,
            },
        )
        .unwrap();
        // Sign of larger magnitude (positive 3.0) wins. We average the
        // surviving same-sign values → just 3.0, scaled by alpha=1 → 3.0.
        assert!((out[0] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn ties_merge_alpha_scales_output() {
        let a = vec![2.0_f32];
        let out = ties_merge(
            &[&a],
            TiesParams {
                keep_top: 0.0,
                alpha: 0.5,
            },
        )
        .unwrap();
        assert!((out[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn ties_merge_rejects_mismatched_lengths() {
        let a = vec![1.0_f32, 2.0];
        let b = vec![1.0_f32];
        assert!(ties_merge(&[&a, &b], TiesParams::default()).is_err());
    }

    #[test]
    fn ties_merge_rejects_empty_input() {
        let r: pf_core::Result<Vec<f32>> = ties_merge(&[], TiesParams::default());
        assert!(r.is_err());
    }

    #[test]
    fn trim_bottom_clears_smallest_magnitudes() {
        let v = vec![0.1_f32, -0.2, 5.0, -8.0];
        // Quantile 0.5 → cut threshold around |0.2|; values <= 0.2 zero out.
        let t = trim_bottom(&v, 0.5);
        // Trimmed entries are written as the literal 0.0_f32, so the
        // bit-equality is precise here despite clippy::float_cmp.
        assert!(t[0] == 0.0);
        assert!(t[1] == 0.0);
        assert!(t[2].abs() > 0.0);
        assert!(t[3].abs() > 0.0);
    }
}
