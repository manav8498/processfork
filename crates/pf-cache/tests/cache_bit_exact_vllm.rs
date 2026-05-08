// SPDX-License-Identifier: MIT
//! Skeleton for an in-process bit-exact replay test against vLLM.
//!
//! As of v1.0.x this is **not** the validation path — the actual
//! vLLM bit-exact validation runs on Modal via
//! `scripts/gpu-validate-modal.py`, and its output lands in
//! `benchmarks/gpu-validation/*.json`. Latest result (Modal A10G,
//! TinyLlama-1.1B, V0 engine): `bit_exact: true` over 38 619 KV
//! pages with byte-identical regenerated text.
//!
//! This test still exists because it documents the shape we want
//! a self-contained local PF_HAS_GPU=1 path to take eventually
//! (subprocess vLLM → snapshot → SIGKILL → fresh worker →
//! checkout → assert bit-equal logits). When that path lands,
//! replace the explicit-skip body with the real subprocess flow.
//! The on-host proxy that DOES run everywhere is
//! `tests/cache_round_trip.rs`.

#[test]
fn vllm_bit_exact_replay() {
    // Always skip cleanly — this is a documentation skeleton, not
    // a validation path. Setting PF_HAS_GPU=1 used to panic here
    // ("not yet wired"); that was misleading because the validation
    // was always meant to run on the Modal lane, not locally.
    eprintln!(
        "skipping: cache_bit_exact_vllm is a v1.0.x documentation \
         skeleton. The real vLLM bit-exact validation runs on Modal \
         (see scripts/gpu-validate-modal.py and \
         benchmarks/gpu-validation/*.json). The on-host proxy that \
         runs everywhere is tests/cache_round_trip.rs."
    );
}
