// SPDX-License-Identifier: MIT
//! Synthetic four-layer capture fixtures.
//!
//! These let us exercise the snapshot orchestrator on the build host (no GPU,
//! no LLM, no CRIU) with realistic-shaped payloads. Used by Phase-1
//! microbenchmarks and the `examples/01-hello-fork/` example.

use crate::digest::Digest256;
use crate::error::Result;
use crate::manifest::{CacheLayer, EffectsLayer, ModelLayer, TraceLayer, WorldLayer};
use crate::snapshot::{LayerCapture, LayerDescriptor, LayerKind};
use crate::store::PfStore;

/// Tunable knobs for the fixture set. Defaults sized for a sub-500-ms run on
/// macOS arm64 (the build host). CI / GPU hosts override `cache_pages` and
/// `model_diff_bytes` upward.
#[derive(Clone, Debug)]
pub struct FixtureSpec {
    /// Number of paged KV-cache pages to emit (each `page_bytes` long).
    pub cache_pages: usize,
    /// Bytes per cache page (default 32 KiB ≈ realistic vLLM page size).
    pub page_bytes: usize,
    /// Number of files to emit in the world-layer FS tree.
    pub world_files: usize,
    /// Bytes per world file.
    pub world_file_bytes: usize,
    /// Bytes of the (single) model-diff blob.
    pub model_diff_bytes: usize,
    /// Number of effect-ledger entries.
    pub effects_entries: usize,
    /// Number of trace messages.
    pub trace_messages: usize,
    /// Optional seed mixed into payloads to differentiate forks.
    pub seed: u64,
}

impl Default for FixtureSpec {
    fn default() -> Self {
        // Tuned so the full snapshot completes well under 500 ms on macOS
        // arm64 with zstd-19 (≈ 1 MB total compressed).
        Self {
            cache_pages: 32,
            page_bytes: 16 * 1024,
            world_files: 64,
            world_file_bytes: 4 * 1024,
            model_diff_bytes: 64 * 1024,
            effects_entries: 16,
            trace_messages: 16,
            seed: 0,
        }
    }
}

/// Cheap deterministic byte generator. Not cryptographic; just gives us
/// payloads that compress like real data (high entropy at the per-byte level)
/// while being reproducible for tests.
fn fill(buf: &mut [u8], seed: u64) {
    // SplitMix64 — 7 ops per 8 bytes, pure stdlib.
    let mut s = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    for chunk in buf.chunks_mut(8) {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let bytes = z.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

// ---------- per-layer fixture captures ----------

/// Synthetic model-layer capture: writes one base + one diff blob.
pub struct FixtureModelCapture(pub FixtureSpec);
impl LayerCapture for FixtureModelCapture {
    fn kind(&self) -> LayerKind {
        LayerKind::Model
    }
    fn capture(&self, store: &PfStore) -> Result<LayerDescriptor> {
        // Base is a fixed marker (HF model fingerprint stand-in) so it dedupes
        // across forks of the same base model.
        let base = store
            .blobs()
            .put(b"base-model-fingerprint:llama-3-8b@sha256:demo")?;
        let mut diff = vec![0u8; self.0.model_diff_bytes];
        fill(&mut diff, self.0.seed ^ 0xD1FF);
        let diff = store.blobs().put(&diff)?;
        Ok(LayerDescriptor::Model(ModelLayer { base, diff }))
    }
}

/// Synthetic cache-layer capture: writes N pages and a manifest blob.
pub struct FixtureCacheCapture(pub FixtureSpec);
impl LayerCapture for FixtureCacheCapture {
    fn kind(&self) -> LayerKind {
        LayerKind::Cache
    }
    fn capture(&self, store: &PfStore) -> Result<LayerDescriptor> {
        let mut page_digests: Vec<(usize, Digest256, Digest256)> =
            Vec::with_capacity(self.0.cache_pages);
        let mut buf_k = vec![0u8; self.0.page_bytes];
        let mut buf_v = vec![0u8; self.0.page_bytes];
        for ix in 0..self.0.cache_pages {
            fill(&mut buf_k, self.0.seed ^ (ix as u64) ^ 0xCACE_CAFE_CAFE);
            fill(
                &mut buf_v,
                self.0.seed ^ (ix as u64) ^ 0xC0DE_C0DE_C0DE_C0DE,
            );
            let k = store.blobs().put(&buf_k)?;
            let v = store.blobs().put(&buf_v)?;
            page_digests.push((ix, k, v));
        }
        // The cache.manifest blob describes the page layout. Ad-hoc JSON for
        // the fixture; real format documented in agent_docs/cache-layer.md.
        let pages_json: Vec<serde_json::Value> = page_digests
            .iter()
            .map(|(ix, k, v)| serde_json::json!({"ix": ix, "k": k.as_str(), "v": v.as_str()}))
            .collect();
        let manifest_json = serde_json::json!({
            "layout": "paged-batchinvariant-v1",
            "page_size_bytes": self.0.page_bytes,
            "pages": pages_json,
        });
        let manifest = store
            .blobs()
            .put(serde_json::to_vec(&manifest_json)?.as_slice())?;
        Ok(LayerDescriptor::Cache(CacheLayer {
            layout: "paged-batchinvariant-v1".into(),
            manifest,
        }))
    }
}

/// Synthetic world-layer capture: writes N file blobs + an env blob + a procs
/// blob (placeholder), then a tree-manifest blob.
pub struct FixtureWorldCapture(pub FixtureSpec);
impl LayerCapture for FixtureWorldCapture {
    fn kind(&self) -> LayerKind {
        LayerKind::World
    }
    fn capture(&self, store: &PfStore) -> Result<LayerDescriptor> {
        let mut buf = vec![0u8; self.0.world_file_bytes];
        let mut entries = Vec::with_capacity(self.0.world_files);
        for i in 0..self.0.world_files {
            fill(&mut buf, self.0.seed ^ (i as u64) ^ 0xF11E_CAFE);
            let d = store.blobs().put(&buf)?;
            entries.push(serde_json::json!({
                "path": format!("src/file_{i:04}.rs"),
                "size": self.0.world_file_bytes,
                "blob": d.as_str(),
            }));
        }
        let tree_json = serde_json::json!({
            "kind": "fs.tree.v1",
            "entries": entries,
        });
        let fs = store.blobs().put(&serde_json::to_vec(&tree_json)?)?;
        let env = store.blobs().put(
            serde_json::to_vec(&serde_json::json!({"PWD":"/sandbox","seed":self.0.seed}))?
                .as_slice(),
        )?;
        let procs = store.blobs().put(
            serde_json::to_vec(&serde_json::json!({"unsupported_on": std::env::consts::OS}))?
                .as_slice(),
        )?;
        Ok(LayerDescriptor::World(WorldLayer { fs, env, procs }))
    }
}

/// Synthetic effects-layer capture: writes a JSONL ledger blob.
pub struct FixtureEffectsCapture(pub FixtureSpec);
impl LayerCapture for FixtureEffectsCapture {
    fn kind(&self) -> LayerKind {
        LayerKind::Effects
    }
    fn capture(&self, store: &PfStore) -> Result<LayerDescriptor> {
        let mut jsonl = Vec::new();
        for i in 0..self.0.effects_entries {
            let entry = serde_json::json!({
                "ts": "2026-05-05T14:11:00Z",
                "tool_id": format!("synth_tool_{}", i % 4),
                "args_hash": format!("sha256:{:064x}", (self.0.seed ^ (i as u64))),
                "idempotency_key": format!("01J{:013}", i),
                "result_hash": format!("sha256:{:064x}", (self.0.seed.wrapping_mul(7) ^ (i as u64))),
                "side_effect_class": if i % 5 == 0 { "irreversible" } else { "pure" },
            });
            jsonl.extend_from_slice(&serde_json::to_vec(&entry)?);
            jsonl.push(b'\n');
        }
        let ledger = store.blobs().put(&jsonl)?;
        Ok(LayerDescriptor::Effects(EffectsLayer { ledger }))
    }
}

/// Synthetic trace-layer capture: writes a JSONL message blob.
pub struct FixtureTraceCapture(pub FixtureSpec);
impl LayerCapture for FixtureTraceCapture {
    fn kind(&self) -> LayerKind {
        LayerKind::Trace
    }
    fn capture(&self, store: &PfStore) -> Result<LayerDescriptor> {
        let mut jsonl = Vec::new();
        for i in 0..self.0.trace_messages {
            let entry = serde_json::json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": format!("synthetic message #{i} (seed {})", self.0.seed),
            });
            jsonl.extend_from_slice(&serde_json::to_vec(&entry)?);
            jsonl.push(b'\n');
        }
        let messages = store.blobs().put(&jsonl)?;
        Ok(LayerDescriptor::Trace(TraceLayer { messages }))
    }
}
