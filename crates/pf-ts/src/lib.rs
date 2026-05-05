// SPDX-License-Identifier: MIT
//! TypeScript bindings for ProcessFork. Phase 0 scaffold; real surface lands
//! in Phase 7.

#![deny(clippy::all)]

use napi_derive::napi;

/// `digestOf(bytes: Buffer): string` — sanity-check that the binding compiles.
///
/// `napi-rs` 2.x extracts `Buffer` into `Vec<u8>`; clippy prefers `&[u8]`
/// for the borrow case but the macro-generated extractor doesn't support it.
/// Documented in `.claude/skills/napi-binding/SKILL.md`.
#[napi]
#[allow(clippy::needless_pass_by_value)]
pub fn digest_of(bytes: Vec<u8>) -> String {
    pf_core::digest::Digest256::of(&bytes).as_str().to_owned()
}
