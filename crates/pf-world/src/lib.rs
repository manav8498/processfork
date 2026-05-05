// SPDX-License-Identifier: MIT
//! # `pf-world`
//!
//! Captures and restores the world a sandboxed agent inhabits: the
//! filesystem, environment variables, in-flight subprocess state (Linux
//! only via CRIU), and — eventually — an attached browser DOM via CDP.
//!
//! See `agent_docs/world-layer.md` for the spec.
//!
//! ## What ships in Phase 2 (this commit)
//!
//! - [`WalkFsCapture`]: portable rayon-parallel file walker that
//!   content-addresses every file into a [`pf_core::cas::BlobStore`] and
//!   emits a tree manifest. Restorable via [`restore_tree`].
//! - [`EnvCapture`]: serializes `std::env::vars()` + cwd, with
//!   `--scrub-env <regex>` redaction.
//! - [`ProcsCapture`]: CRIU dump on Linux (shellout); on every other host a
//!   self-describing JSON placeholder per `agent_docs/world-layer.md`.
//!
//! ## What's planned but stubbed
//!
//! - macOS APFS clone fast-path via `clonefile(2)`: when the source dir
//!   sits on APFS, `WalkFsCapture::with_apfs_clone` first clones the dir
//!   in O(1), then walks the clone — giving a stable read-snapshot without
//!   pausing the agent. Wired but not enabled by default; see
//!   [`WalkFsCapture::use_apfs_clone`].
//! - Linux overlayfs upper-dir capture: deferred to v1.1 (documented in
//!   the v2 list of `claude-progress.json`).

// `forbid(unsafe_code)` is too strict because `std::env::set_var` is unsafe
// since Rust 1.85 — we need it in the env-capture tests. We use `deny`
// (overridable) at the crate level and locally allow only in the test
// modules that touch process-global env state.
#![deny(unsafe_code)]
#![allow(missing_docs)] // docs added per-symbol in submodules

pub mod env;
pub mod fs;
pub mod procs;

pub use env::EnvCapture;
pub use fs::{FsTree, FsTreeEntry, WalkFsCapture, restore_tree};
pub use procs::ProcsCapture;
