// SPDX-License-Identifier: MIT
//! # `pf-effects`
//!
//! Append-only ledger of every irreversible tool call an agent makes, with
//! per-call idempotency keys and an HMAC chain that defends against
//! semantic-rollback attacks (ACRFence, arXiv 2603.20625). See
//! `agent_docs/effects-layer.md` for the spec.

#![deny(unsafe_code)]
#![allow(missing_docs)]

pub mod ledger;
pub mod policy;
pub mod proxy;

pub use ledger::{Ledger, LedgerEntry, SessionSecret, SideEffectClass};
pub use policy::{ReplayDecision, ReplayPolicy};
pub use proxy::{ToolHandler, ToolProxy};
