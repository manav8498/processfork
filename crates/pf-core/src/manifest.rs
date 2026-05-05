// SPDX-License-Identifier: MIT
//! `.pfimg` manifest format (schema v1).
//!
//! See `agent_docs/architecture.md` §4.2 for the canonical schema.

use crate::digest::Digest256;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Wire mediatype for the v1 manifest.
pub const MEDIATYPE_V1: &str = "application/vnd.processfork.image.v1+json";

/// Top-level `.pfimg` manifest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest schema version. Always `1` for now.
    pub schema_version: u32,
    /// OCI-style mediatype, see [`MEDIATYPE_V1`].
    pub media_type: String,
    /// Which agent kind produced this image (`claude-code`, `langgraph`, …).
    pub agent: AgentInfo,
    /// Pointers to the four content-addressed layer blobs.
    pub model: ModelLayer,
    /// KV-cache layer descriptor.
    pub cache: CacheLayer,
    /// World layer descriptor (FS, env, processes).
    pub world: WorldLayer,
    /// Effect-ledger descriptor.
    pub effects: EffectsLayer,
    /// Reasoning trace (chat messages, tool-call log).
    pub trace: TraceLayer,
    /// When this image was sealed.
    pub created_at: DateTime<Utc>,
    /// Parent image digests (zero, one, or two — two means a merge).
    #[serde(default)]
    pub parents: Vec<Digest256>,
}

/// Identifies the agent runtime that produced this image.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Runtime kind (`claude-code`, `langgraph`, `vllm`, …).
    pub kind: String,
    /// Runtime version string.
    pub version: String,
    /// Opaque fingerprint of the runtime build (used to gate restore).
    pub fingerprint: String,
}

/// Pointers to the model-layer blobs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelLayer {
    /// Digest of the base-model weights (often shared across many images).
    pub base: Digest256,
    /// Digest of the diff blob (LoRA / IA³ / full delta).
    pub diff: Digest256,
}

/// Pointers to the KV-cache layer blobs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheLayer {
    /// On-disk page layout identifier; default `paged-batchinvariant-v1`.
    pub layout: String,
    /// Digest of the page-manifest blob.
    pub manifest: Digest256,
}

/// Pointers to the world-layer (FS / env / processes) blobs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldLayer {
    /// Digest of the filesystem snapshot.
    pub fs: Digest256,
    /// Digest of the captured environment (env vars + cwd).
    pub env: Digest256,
    /// Digest of the captured in-flight process state (CRIU dump).
    pub procs: Digest256,
}

/// Pointer to the effects-ledger blob.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EffectsLayer {
    /// Digest of the append-only effect ledger.
    pub ledger: Digest256,
}

/// Pointer to the reasoning-trace blob.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceLayer {
    /// Digest of the chat / tool-call message log.
    pub messages: Digest256,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_through_json() {
        let d = Digest256::of(b"x");
        let m = Manifest {
            schema_version: 1,
            media_type: MEDIATYPE_V1.to_owned(),
            agent: AgentInfo {
                kind: "claude-code".into(),
                version: "0.1.0".into(),
                fingerprint: "test".into(),
            },
            model: ModelLayer {
                base: d.clone(),
                diff: d.clone(),
            },
            cache: CacheLayer {
                layout: "paged-batchinvariant-v1".into(),
                manifest: d.clone(),
            },
            world: WorldLayer {
                fs: d.clone(),
                env: d.clone(),
                procs: d.clone(),
            },
            effects: EffectsLayer { ledger: d.clone() },
            trace: TraceLayer {
                messages: d.clone(),
            },
            created_at: Utc::now(),
            parents: vec![],
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.schema_version, 1);
        assert_eq!(back.media_type, MEDIATYPE_V1);
    }
}
