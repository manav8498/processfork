// SPDX-License-Identifier: MIT
//! Replay-or-fork policy enforcement.

use crate::ledger::{LedgerEntry, SideEffectClass};
use serde::{Deserialize, Serialize};

/// Per-class policy for what to do with a previously-recorded effect on
/// restore.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplayMode {
    /// Inject the cached `result_hash` payload as if the call just returned.
    /// Safe for `Pure`, `Idempotent`, and `NetworkOnly` by default.
    InjectCachedResult,
    /// Re-issue the underlying tool with the same `idempotency_key`. Safe
    /// only for tools the author explicitly classified `Idempotent`.
    ReplayWithSameKey,
    /// Mint a new `idempotency_key` and re-issue. **Dangerous** — a new email
    /// will be sent / a new charge made. Only when operator opts in.
    ReplayWithNewKey,
    /// Surface the previous effect as an immutable fact; do nothing on this
    /// restore. The agent's prompt sees the result but the tool isn't called.
    SurfaceAsFact,
}

/// The aggregate decision for one ledger entry under a [`ReplayPolicy`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayDecision {
    /// Which mode to use.
    pub mode: ReplayMode,
    /// Human-readable rationale; surfaced in `pf checkout`'s output.
    pub rationale: &'static str,
}

/// User-supplied policy mapping side-effect classes → replay modes.
///
/// Defaults match `agent_docs/effects-layer.md`:
///
/// | class          | default mode             |
/// |----------------|--------------------------|
/// | `Pure`         | `InjectCachedResult`     |
/// | `Idempotent`   | `InjectCachedResult`     |
/// | `Irreversible` | `SurfaceAsFact`          |
/// | `NetworkOnly`  | `InjectCachedResult`     |
#[derive(Clone, Copy, Debug)]
pub struct ReplayPolicy {
    pub on_pure: ReplayMode,
    pub on_idempotent: ReplayMode,
    pub on_irreversible: ReplayMode,
    pub on_network_only: ReplayMode,
}

impl Default for ReplayPolicy {
    fn default() -> Self {
        Self {
            on_pure: ReplayMode::InjectCachedResult,
            on_idempotent: ReplayMode::InjectCachedResult,
            on_irreversible: ReplayMode::SurfaceAsFact,
            on_network_only: ReplayMode::InjectCachedResult,
        }
    }
}

impl ReplayPolicy {
    /// Strict: `SurfaceAsFact` for everything except `Pure`. Use when restoring
    /// snapshots from untrusted sources.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            on_pure: ReplayMode::InjectCachedResult,
            on_idempotent: ReplayMode::SurfaceAsFact,
            on_irreversible: ReplayMode::SurfaceAsFact,
            on_network_only: ReplayMode::SurfaceAsFact,
        }
    }

    /// Aggressive: re-issue idempotent tools with same key, re-issue
    /// irreversible tools with NEW keys (charges card again, etc.). Only
    /// for explicit operator opt-in via `pf merge --replay-effects=all`.
    #[must_use]
    pub const fn aggressive() -> Self {
        Self {
            on_pure: ReplayMode::InjectCachedResult,
            on_idempotent: ReplayMode::ReplayWithSameKey,
            on_irreversible: ReplayMode::ReplayWithNewKey,
            on_network_only: ReplayMode::ReplayWithSameKey,
        }
    }

    /// Decide what to do with a single entry.
    #[must_use]
    pub const fn decide(&self, entry: &LedgerEntry) -> ReplayDecision {
        let mode = match entry.side_effect_class {
            SideEffectClass::Pure => self.on_pure,
            SideEffectClass::Idempotent => self.on_idempotent,
            SideEffectClass::Irreversible => self.on_irreversible,
            SideEffectClass::NetworkOnly => self.on_network_only,
        };
        let rationale = match (entry.side_effect_class, mode) {
            (SideEffectClass::Irreversible, ReplayMode::SurfaceAsFact) => {
                "irreversible effect: surfacing prior result as a cached fact"
            }
            (SideEffectClass::Irreversible, ReplayMode::ReplayWithNewKey) => {
                "DANGER: re-issuing irreversible tool with new key (--replay-effects=all)"
            }
            (_, ReplayMode::InjectCachedResult) => {
                "side-effect class is replay-safe: injecting cached result"
            }
            (_, ReplayMode::ReplayWithSameKey) => {
                "re-issuing tool with same idempotency key (idempotent semantics)"
            }
            _ => "policy decision",
        };
        ReplayDecision { mode, rationale }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::SessionSecret;
    use chrono::Utc;
    use pf_core::digest::Digest256;

    fn entry(class: SideEffectClass) -> LedgerEntry {
        LedgerEntry {
            timestamp: Utc::now(),
            tool_id: "t".into(),
            args_hash: Digest256::of(b""),
            idempotency_key: "k".into(),
            result_hash: Digest256::of(b""),
            side_effect_class: class,
            session_hmac: String::new(),
        }
    }

    #[test]
    fn default_never_re_issues_irreversible() {
        let p = ReplayPolicy::default();
        assert_eq!(
            p.decide(&entry(SideEffectClass::Irreversible)).mode,
            ReplayMode::SurfaceAsFact
        );
    }

    #[test]
    fn strict_surfaces_idempotent_too() {
        let p = ReplayPolicy::strict();
        assert_eq!(
            p.decide(&entry(SideEffectClass::Idempotent)).mode,
            ReplayMode::SurfaceAsFact
        );
    }

    #[test]
    fn aggressive_re_issues_irreversible() {
        let p = ReplayPolicy::aggressive();
        assert_eq!(
            p.decide(&entry(SideEffectClass::Irreversible)).mode,
            ReplayMode::ReplayWithNewKey
        );
    }

    #[test]
    fn touches_session_secret_only_for_construction() {
        // Sanity that SessionSecret is construct-only; policy doesn't need it.
        let _ = SessionSecret::new(b"x".to_vec());
    }
}
