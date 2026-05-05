// SPDX-License-Identifier: MIT
//! Effects-layer merge: emit an `effects.merged.v1` blob that references
//! both parent ledgers + the replay-class policy.
//!
//! Per `agent_docs/merge-protocol.md` we **never** forge new HMAC
//! signatures over a re-chained ledger — that would either require
//! sharing the original session secret (it's per-session by design) or
//! breaking the chain. Instead the merged blob carries pointers to both
//! parent ledgers; on restore the [`pf_effects::ToolProxy`] reads both
//! per the replay policy.

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_effects::SideEffectClass;
use serde::{Deserialize, Serialize};

/// Wire format for a merged effects blob.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergedEffectsLedger {
    /// Schema discriminator. Always `"effects.merged.v1"`.
    pub kind: String,
    /// Digest of A's original ledger (`effects.ledger.v1`).
    pub parent_a: Digest256,
    /// Digest of B's original ledger.
    pub parent_b: Digest256,
    /// Digest of the common-ancestor's ledger (informational; the engine
    /// uses this for diffing in `pf merge --tool`).
    pub ancestor: Digest256,
    /// Which classes were authorized for replay-with-new-key on this
    /// merge. `Irreversible` SHOULD only be in this list when the operator
    /// passed `--replay-effects=all`.
    pub replay_with_new_key: Vec<SideEffectClass>,
    /// Pre-computed counts so `pf merge` UX can print them without
    /// re-walking the parent ledgers.
    pub summary: ReplayPolicySummary,
}

/// Counts surfaced in the `pf merge` UX line.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ReplayPolicySummary {
    /// Entries from A.
    pub a_entries: u64,
    /// Entries from B.
    pub b_entries: u64,
    /// Entries from B that are `Irreversible` and will be cached as
    /// facts (the agent's prompt sees the prior result; the tool is NOT
    /// re-issued).
    pub b_irreversible_cached: u64,
}

/// Compose a merged effects blob. Reads both parent ledgers (and the
/// ancestor's) into RAM, counts entries by class, writes the wrapper.
///
/// `replay_with_new_key` lists side-effect classes the operator authorized
/// for re-issue. The default policy passes an empty `Vec`, which means
/// everything from B that's `Irreversible` will be surfaced as a cached
/// fact.
pub fn merge_effects(
    blobs: &dyn BlobStore,
    a: &Digest256,
    b: &Digest256,
    ancestor: &Digest256,
    replay_with_new_key: Vec<SideEffectClass>,
) -> pf_core::Result<Digest256> {
    let a_entries = count_ledger_entries(blobs, a)?;
    let (b_entries, b_irreversible) = count_ledger_entries_with_class(blobs, b)?;

    let merged = MergedEffectsLedger {
        kind: "effects.merged.v1".into(),
        parent_a: a.clone(),
        parent_b: b.clone(),
        ancestor: ancestor.clone(),
        replay_with_new_key,
        summary: ReplayPolicySummary {
            a_entries,
            b_entries,
            b_irreversible_cached: b_irreversible,
        },
    };
    blobs.put(&serde_json::to_vec(&merged)?)
}

/// Read an `effects.ledger.v1` blob and return the number of entries.
fn count_ledger_entries(blobs: &dyn BlobStore, digest: &Digest256) -> pf_core::Result<u64> {
    let bytes = blobs.get(digest)?;
    let mut total = 0_u64;
    let mut seen_header = false;
    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if !seen_header {
            // First non-empty line is the header (`{"kind":"effects.ledger.v1",…}`).
            // Validate but don't count.
            let header: serde_json::Value = serde_json::from_slice(line)?;
            if header.get("kind").and_then(|v| v.as_str()) != Some("effects.ledger.v1") {
                return Err(pf_core::Error::Integrity(format!(
                    "expected effects.ledger.v1, got {header:?}"
                )));
            }
            seen_header = true;
            continue;
        }
        total += 1;
    }
    Ok(total)
}

/// Same as above but also counts how many entries are `Irreversible`.
fn count_ledger_entries_with_class(
    blobs: &dyn BlobStore,
    digest: &Digest256,
) -> pf_core::Result<(u64, u64)> {
    let bytes = blobs.get(digest)?;
    let mut total = 0_u64;
    let mut irreversible = 0_u64;
    let mut seen_header = false;
    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if !seen_header {
            let header: serde_json::Value = serde_json::from_slice(line)?;
            if header.get("kind").and_then(|v| v.as_str()) != Some("effects.ledger.v1") {
                return Err(pf_core::Error::Integrity(format!(
                    "expected effects.ledger.v1, got {header:?}"
                )));
            }
            seen_header = true;
            continue;
        }
        total += 1;
        let entry: serde_json::Value = serde_json::from_slice(line)?;
        if entry.get("side_effect_class").and_then(|v| v.as_str()) == Some("irreversible") {
            irreversible += 1;
        }
    }
    Ok((total, irreversible))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use pf_core::cas::MemBlobStore;
    use pf_core::digest::Digest256 as D;
    use pf_effects::{Ledger, SessionSecret, SideEffectClass};

    fn ledger_with(n_pure: u32, n_irr: u32, secret: &[u8]) -> Ledger {
        let mut l = Ledger::new(SessionSecret::new(secret.to_vec()));
        for i in 0..n_pure {
            l.append(
                Utc::now(),
                "p",
                D::of(&[i as u8; 32]),
                format!("pk{i}"),
                D::of(&[i as u8 ^ 1; 32]),
                SideEffectClass::Pure,
            )
            .unwrap();
        }
        for i in 0..n_irr {
            l.append(
                Utc::now(),
                "send_email",
                D::of(&[(i + 100) as u8; 32]),
                format!("ik{i}"),
                D::of(&[(i + 100) as u8 ^ 1; 32]),
                SideEffectClass::Irreversible,
            )
            .unwrap();
        }
        l
    }

    #[test]
    fn merged_blob_carries_correct_counts() {
        let blobs = MemBlobStore::new();
        let a = ledger_with(3, 1, b"sa").serialize(&blobs).unwrap();
        let b = ledger_with(2, 4, b"sb").serialize(&blobs).unwrap();
        let x = ledger_with(1, 0, b"sx").serialize(&blobs).unwrap();

        let cid = merge_effects(&blobs, &a, &b, &x, vec![]).unwrap();
        let bytes = blobs.get(&cid).unwrap();
        let merged: MergedEffectsLedger = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(merged.kind, "effects.merged.v1");
        assert_eq!(merged.summary.a_entries, 4); // 3 pure + 1 irr
        assert_eq!(merged.summary.b_entries, 6); // 2 pure + 4 irr
        assert_eq!(merged.summary.b_irreversible_cached, 4);
        assert!(merged.replay_with_new_key.is_empty());
    }

    #[test]
    fn replay_classes_round_trip() {
        let blobs = MemBlobStore::new();
        let a = ledger_with(0, 0, b"sa").serialize(&blobs).unwrap();
        let b = ledger_with(0, 0, b"sb").serialize(&blobs).unwrap();
        let x = ledger_with(0, 0, b"sx").serialize(&blobs).unwrap();
        let cid = merge_effects(
            &blobs,
            &a,
            &b,
            &x,
            vec![SideEffectClass::Idempotent, SideEffectClass::NetworkOnly],
        )
        .unwrap();
        let bytes = blobs.get(&cid).unwrap();
        let merged: MergedEffectsLedger = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            merged.replay_with_new_key,
            vec![SideEffectClass::Idempotent, SideEffectClass::NetworkOnly]
        );
    }

    #[test]
    fn rejects_non_ledger_blob() {
        let blobs = MemBlobStore::new();
        let bogus = blobs.put(b"{\"kind\":\"not.a.ledger\"}\n").unwrap();
        let other = ledger_with(0, 0, b"x").serialize(&blobs).unwrap();
        let err = merge_effects(&blobs, &bogus, &other, &other, vec![]).unwrap_err();
        assert!(matches!(err, pf_core::Error::Integrity(_)));
    }
}
