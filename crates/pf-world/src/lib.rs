// SPDX-License-Identifier: MIT
//! # `pf-world`
//!
//! Captures and restores the world a sandboxed agent inhabits: filesystem,
//! environment variables, in-flight subprocess state, and (optionally) a
//! browser DOM via the Chrome DevTools Protocol. Phase 0 scaffold only.

#![forbid(unsafe_code)]

/// Filesystem backends we know how to snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsBackend {
    /// Linux overlayfs lower+upper layers.
    Overlayfs,
    /// macOS APFS clone (`clonefile(2)`).
    ApfsClone,
}
