// SPDX-License-Identifier: MIT
//! # `pf-core`
//!
//! Core primitives for ProcessFork: the content-addressed store (CAS), the
//! `.pfimg` manifest format, and atomic-snapshot orchestration across the four
//! layers (model, cache, world, effects).
//!
//! Phase 0: scaffold only. Real implementation lands in Phase 1.

#![forbid(unsafe_code)]

pub mod cas;
pub mod digest;
pub mod error;
pub mod fixture;
pub mod manifest;
pub mod snapshot;
pub mod store;

pub use error::{Error, Result};
