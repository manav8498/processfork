// SPDX-License-Identifier: MIT
//! # `pf-effects`
//!
//! Append-only ledger of every irreversible tool call an agent makes, with
//! idempotency keys and replay-or-fork policy enforcement per ACRFence
//! semantics. Phase 0 scaffold only.

#![forbid(unsafe_code)]

/// How "dangerous" a tool call's side-effect is, for replay policy purposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SideEffectClass {
    /// Pure function — safe to replay from cached result.
    Pure,
    /// Idempotent under the same idempotency key (POST-with-key, PUT, …).
    Idempotent,
    /// Genuinely irreversible (sent email, charged card, dropped table).
    Irreversible,
    /// Network-only read (e.g., GET) — cacheable but stale-aware.
    NetworkOnly,
}
