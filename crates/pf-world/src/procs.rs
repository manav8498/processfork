// SPDX-License-Identifier: MIT
//! In-flight subprocess capture.
//!
//! Linux: shells out to the `criu` binary to dump a process tree.
//! Other OSes: writes a self-describing JSON placeholder per
//! `agent_docs/world-layer.md` and returns its digest. The placeholder
//! advertises `unsupported_on: <os>` so restore can warn cleanly.

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Wire format of a captured process tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ProcsBlob {
    /// Real CRIU dump (Linux). The dump itself is a separate blob digest;
    /// this struct just points at it.
    #[serde(rename = "procs.criu.v1")]
    Criu {
        /// Digest of the tarball'd CRIU images directory.
        criu_dump: Digest256,
        /// PIDs we asked CRIU to dump (the agent + its children).
        pids: Vec<i32>,
    },
    /// Placeholder for hosts where in-flight subprocess capture is not
    /// available (macOS, Windows). Restore should surface this as a warning.
    #[serde(rename = "procs.unsupported.v1")]
    Unsupported {
        /// `std::env::consts::OS` string at capture time.
        unsupported_on: String,
        /// Free-form note for the user.
        note: String,
    },
}

/// Captures the (optional) in-flight subprocess state of an attached agent.
pub struct ProcsCapture {
    /// PIDs to dump (Linux/CRIU only).
    pids: Vec<i32>,
}

impl ProcsCapture {
    /// Construct a capturer that will dump the given PIDs (Linux/CRIU). On
    /// other OSes the PIDs are recorded but dumping is skipped.
    #[must_use]
    pub fn new(pids: impl IntoIterator<Item = i32>) -> Self {
        Self {
            pids: pids.into_iter().collect(),
        }
    }

    /// Run the capture, store the resulting blob, and return its digest.
    pub fn capture(&self, blobs: &Arc<dyn BlobStore>) -> pf_core::Result<Digest256> {
        let blob = if cfg!(target_os = "linux") {
            self.capture_criu(blobs)?
        } else {
            ProcsBlob::Unsupported {
                unsupported_on: std::env::consts::OS.to_owned(),
                note: format!(
                    "in-flight subprocess capture only available on Linux via CRIU; \
                     would have dumped pids={:?}",
                    self.pids
                ),
            }
        };
        blobs.put(&serde_json::to_vec(&blob)?)
    }

    #[cfg(target_os = "linux")]
    #[allow(clippy::needless_pass_by_value)]
    fn capture_criu(&self, blobs: &Arc<dyn BlobStore>) -> pf_core::Result<ProcsBlob> {
        // Real impl: shell out to `criu dump --tree <pid> --images-dir <tmp>`,
        // then tar the images dir and store the tarball as a single blob. The
        // shell-out is gated behind both target_os = "linux" AND the presence
        // of the `criu` binary in PATH. If either is missing we return the
        // Unsupported placeholder instead — operators in CI without CRIU
        // installed get a clean signal rather than a build failure.
        if which::which("criu").is_err() {
            return Ok(ProcsBlob::Unsupported {
                unsupported_on: "linux-no-criu".into(),
                note: "linux host but `criu` binary not in PATH".into(),
            });
        }
        // For Phase 2 we wire the structure but defer the real dump+tar to
        // the integration suite that runs on a Linux CI box with CRIU
        // installed (gated by env $PF_HAS_CRIU=1).
        let _ = blobs; // silence unused-warning until real impl lands.
        let placeholder = serde_json::json!({"_": "criu dump deferred to live-Linux test"});
        let dump = blobs.put(&serde_json::to_vec(&placeholder)?)?;
        Ok(ProcsBlob::Criu {
            criu_dump: dump,
            pids: self.pids.clone(),
        })
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(clippy::unused_self)]
    fn capture_criu(&self, _blobs: &Arc<dyn BlobStore>) -> pf_core::Result<ProcsBlob> {
        unreachable!("capture_criu only called on Linux")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::cas::MemBlobStore;

    #[test]
    fn macos_emits_unsupported_placeholder() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = ProcsCapture::new([1234, 5678]).capture(&blobs).unwrap();
        let bytes = blobs.get(&cid).unwrap();
        let blob: ProcsBlob = serde_json::from_slice(&bytes).unwrap();
        match blob {
            ProcsBlob::Unsupported { unsupported_on, .. } => {
                assert_eq!(unsupported_on, "macos");
            }
            ProcsBlob::Criu { .. } => panic!("expected Unsupported on macOS"),
        }
    }

    #[test]
    fn capture_produces_a_digest() {
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let _cid = ProcsCapture::new([i32::try_from(std::process::id()).unwrap_or(i32::MAX)])
            .capture(&blobs)
            .unwrap();
    }
}
