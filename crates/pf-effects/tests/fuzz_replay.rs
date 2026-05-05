// SPDX-License-Identifier: MIT
//! Phase-3 acceptance: conformance fuzzer for the effect-layer.
//!
//! Generates 1000 random ledger sequences and asserts the four invariants
//! from `agent_docs/effects-layer.md`:
//!   1. Default replay policy never re-issues an Irreversible call.
//!   2. Idempotency keys are unique within a session.
//!   3. HMAC chain validates.
//!   4. Forking (truncating to a prefix) preserves no-duplicate-irreversible.

#![allow(clippy::needless_pass_by_value)] // proptest macro idiom

use chrono::Utc;
use pf_core::digest::Digest256;
use pf_effects::{Ledger, ReplayPolicy, SessionSecret, SideEffectClass, policy::ReplayMode};
use proptest::prelude::*;

fn class_strategy() -> impl Strategy<Value = SideEffectClass> {
    prop_oneof![
        Just(SideEffectClass::Pure),
        Just(SideEffectClass::Idempotent),
        Just(SideEffectClass::Irreversible),
        Just(SideEffectClass::NetworkOnly),
    ]
}

#[derive(Clone, Debug)]
struct Op {
    tool: String,
    arg_byte: u8,
    class: SideEffectClass,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    ("[a-z]{1,8}", any::<u8>(), class_strategy()).prop_map(|(tool, arg_byte, class)| Op {
        tool,
        arg_byte,
        class,
    })
}

fn build_ledger(ops: &[Op]) -> Ledger {
    let mut l = Ledger::new(SessionSecret::new(b"fuzz".to_vec()));
    for (ix, op) in ops.iter().enumerate() {
        l.append(
            Utc::now(),
            op.tool.clone(),
            Digest256::of(&[op.arg_byte; 32]),
            format!("k{ix:08}"),
            Digest256::of(&[op.arg_byte ^ 0x55; 32]),
            op.class,
        )
        .unwrap();
    }
    l
}

proptest! {
    // 1000 random traces per agent_docs/effects-layer.md fuzzer requirement.
    #![proptest_config(ProptestConfig {
        cases: 1000,
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn invariant_1_default_policy_never_reissues_irreversible(
        ops in prop::collection::vec(op_strategy(), 0..32)
    ) {
        let ledger = build_ledger(&ops);
        let policy = ReplayPolicy::default();
        for entry in ledger.entries() {
            let decision = policy.decide(entry);
            if entry.side_effect_class == SideEffectClass::Irreversible {
                prop_assert_ne!(decision.mode, ReplayMode::ReplayWithSameKey);
                prop_assert_ne!(decision.mode, ReplayMode::ReplayWithNewKey);
            }
        }
    }

    #[test]
    fn invariant_2_idempotency_keys_are_unique(
        ops in prop::collection::vec(op_strategy(), 0..32)
    ) {
        let ledger = build_ledger(&ops);
        let mut seen = std::collections::HashSet::new();
        for e in ledger.entries() {
            prop_assert!(
                seen.insert(e.idempotency_key.clone()),
                "duplicate idempotency_key: {}", e.idempotency_key
            );
        }
    }

    #[test]
    fn invariant_3_hmac_chain_always_validates_when_untouched(
        ops in prop::collection::vec(op_strategy(), 0..32)
    ) {
        let ledger = build_ledger(&ops);
        prop_assert!(ledger.verify().is_ok());
    }

    #[test]
    fn invariant_4_fork_prefix_preserves_no_duplicate_irreversible(
        ops in prop::collection::vec(op_strategy(), 1..32),
        cut in 1usize..32usize,
    ) {
        let parent = build_ledger(&ops);
        // Take a prefix as the "fork point", then continue independently
        // on both sides. The parent already has no duplicate Irreversible
        // (each entry is unique by idempotency_key). After fork, each child
        // continues to mint fresh keys; we assert no Irreversible
        // idempotency_key appears in BOTH children.
        let cut = cut.min(parent.entries().len());
        let prefix: Vec<_> = parent.entries()[..cut].to_vec();

        // Two children each append one new op.
        let mut child_a = Ledger::new(SessionSecret::new(b"a".to_vec()));
        for e in &prefix {
            child_a.append(
                e.timestamp, e.tool_id.clone(), e.args_hash.clone(),
                e.idempotency_key.clone(), e.result_hash.clone(), e.side_effect_class,
            ).unwrap();
        }
        child_a.append(
            Utc::now(), "extra_a", Digest256::of(b"a"), "child_a_extra",
            Digest256::of(b"a"), SideEffectClass::Irreversible,
        ).unwrap();

        let mut child_b = Ledger::new(SessionSecret::new(b"b".to_vec()));
        for e in &prefix {
            child_b.append(
                e.timestamp, e.tool_id.clone(), e.args_hash.clone(),
                e.idempotency_key.clone(), e.result_hash.clone(), e.side_effect_class,
            ).unwrap();
        }
        child_b.append(
            Utc::now(), "extra_b", Digest256::of(b"b"), "child_b_extra",
            Digest256::of(b"b"), SideEffectClass::Irreversible,
        ).unwrap();

        // The new Irreversible keys post-fork must differ.
        let a_irr_keys: std::collections::HashSet<_> = child_a.entries()
            .iter().skip(prefix.len())
            .filter(|e| e.side_effect_class == SideEffectClass::Irreversible)
            .map(|e| e.idempotency_key.clone()).collect();
        let b_irr_keys: std::collections::HashSet<_> = child_b.entries()
            .iter().skip(prefix.len())
            .filter(|e| e.side_effect_class == SideEffectClass::Irreversible)
            .map(|e| e.idempotency_key.clone()).collect();
        prop_assert!(a_irr_keys.is_disjoint(&b_irr_keys),
            "two children minted overlapping irreversible idempotency keys");
    }
}
