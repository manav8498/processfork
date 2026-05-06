// SPDX-License-Identifier: MIT
//! `pf snapshot` — capture an FS sandbox + chat trace into a `.pfimg`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use pf_core::cas::BlobStore;
use pf_core::manifest::{
    AgentInfo, CacheLayer, EffectsLayer, MEDIATYPE_V1, Manifest, ModelLayer, TraceLayer, WorldLayer,
};
use pf_core::store::PfStore;

use super::CliError;

#[derive(Debug, Parser)]
pub struct Args {
    /// User-friendly identifier for the agent (`claude-code`, `langgraph`, …).
    #[arg(long, default_value = "anonymous")]
    pub agent_id: String,

    /// Filesystem root to capture into the world layer.
    #[arg(long)]
    pub fs_root: PathBuf,

    /// Optional human-readable name; recorded in agent.fingerprint.
    #[arg(long, short = 'n')]
    pub name: Option<String>,

    /// Optional JSONL file of chat messages (`{"role":...,"content":...}` per line).
    /// Validated at snapshot time — a missing/unreadable path fails fast
    /// rather than silently capturing an empty trace that later breaks merge.
    #[arg(long)]
    pub trace_from_jsonl: Option<PathBuf>,

    /// Optional JSONL file of tool-call ledger entries (one
    /// `{"timestamp":..., "tool_id":..., "args_hash":..., "idempotency_key":...,
    ///   "result_hash":..., "side_effect_class":...}` per line).
    /// Adapters maintain this file as the agent runs; snapshot folds it
    /// into the world image so restored agents see prior side effects as
    /// facts (ACRFence). Pre-validated at snapshot time.
    #[arg(long)]
    pub effects_from_jsonl: Option<PathBuf>,

    /// Regex of env-var names to redact from the captured environment.
    /// Repeatable. Per spec §4.7 — without this every env var (including
    /// secrets) lands in the world-layer env blob. Recommended baseline:
    /// `--scrub-env '(?i)token|secret|password|key'`.
    #[arg(long)]
    pub scrub_env: Vec<String>,

    /// Parent CIDs to record in `manifest.parents`. Set this when you're
    /// snapshotting after a `pf fork` so `pf merge` can find the common
    /// ancestor. Repeatable.
    #[arg(long)]
    pub parent: Vec<String>,
}

// `pf snapshot run()` accumulated a fair amount of validation +
// layer-assembly logic by v1.0.3 (path-traversal hardening, env
// scrub, effects ledger, parent lineage, .pfcid sentinel,
// trace pre-validation). Splitting it into smaller helpers would
// just shuffle the same surface area through more functions; the
// linear flow is easier to audit as one block.
#[allow(clippy::too_many_lines)]
pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let blobs: Arc<dyn BlobStore> = store.blobs_arc();

    if !args.fs_root.exists() {
        return Err(CliError::BadInput(format!(
            "--fs-root does not exist: {}",
            args.fs_root.display()
        ))
        .into());
    }

    // Pre-validate --trace-from-jsonl so a malformed/missing path
    // fails the snapshot instead of producing an empty trace blob
    // that breaks `pf merge` later (v1.0.2 audit finding).
    for (flag, opt) in [
        ("--trace-from-jsonl", &args.trace_from_jsonl),
        ("--effects-from-jsonl", &args.effects_from_jsonl),
    ] {
        if let Some(p) = opt {
            if !p.exists() {
                return Err(CliError::BadInput(format!(
                    "{flag} path does not exist: {}",
                    p.display()
                ))
                .into());
            }
            if !p.is_file() {
                return Err(CliError::BadInput(format!(
                    "{flag} path is not a regular file: {}",
                    p.display()
                ))
                .into());
            }
        }
    }

    // World — env capture honours --scrub-env so secrets in the
    // shell environment don't end up in the .pfimg. The fs walker
    // opts into the APFS-clone fast-path on macOS by default — the
    // O(1) clonefile(2) gives us a stable read-snapshot so the agent
    // can keep writing without risking the mid-snapshot torn state
    // that the v1.0.2 audit reproduced (a.txt v1, b.txt v0).
    let fs_digest = pf_world::WalkFsCapture::new(&args.fs_root)
        .use_apfs_clone(cfg!(target_os = "macos"))
        .capture(&blobs)?;
    let mut env_capture = pf_world::EnvCapture::new();
    for pat in &args.scrub_env {
        env_capture = env_capture
            .scrub(pat)
            .map_err(|e| CliError::BadInput(format!("--scrub-env regex {pat:?}: {e}")))?;
    }
    let env_digest = env_capture.capture(&blobs)?;
    let procs_blob = serde_json::json!({
        "kind": "procs.unsupported.v1",
        "unsupported_on": std::env::consts::OS,
        "note": "pf snapshot does not capture in-flight subprocesses without the CRIU adapter",
    });
    let procs_digest = blobs.put(&serde_json::to_vec(&procs_blob)?)?;

    // Trace.
    let trace_bytes = if let Some(p) = &args.trace_from_jsonl {
        std::fs::read(p)?
    } else {
        Vec::new()
    };
    let trace_digest = blobs.put(&trace_bytes)?;

    // Effects ledger. If --effects-from-jsonl is supplied, fold each
    // line into the on-disk ledger. The header records the entry count
    // so consumers can sanity-check at restore time.
    let ledger_digest = {
        let mut entries: Vec<serde_json::Value> = Vec::new();
        if let Some(p) = &args.effects_from_jsonl {
            for (lineno, line) in std::fs::read_to_string(p)?.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                    CliError::BadInput(format!(
                        "--effects-from-jsonl line {} is not valid JSON: {e}",
                        lineno + 1
                    ))
                })?;
                entries.push(v);
            }
        }
        let mut body = format!(
            "{{\"kind\":\"effects.ledger.v1\",\"entries\":{}}}\n",
            entries.len()
        )
        .into_bytes();
        for e in entries {
            body.extend_from_slice(serde_json::to_string(&e)?.as_bytes());
            body.push(b'\n');
        }
        blobs.put(&body)?
    };

    // Model (empty Lora envelope).
    let model_envelope = serde_json::json!({
        "layout": "model.diff.v1",
        "diff": {"kind": "lora", "adapters": []},
    });
    let model_diff = blobs.put(&serde_json::to_vec(&model_envelope)?)?;
    let model_base = blobs.put(format!("base:{}", args.agent_id).as_bytes())?;

    // Cache (empty page manifest).
    let cache_envelope = serde_json::json!({
        "layout": "paged-batchinvariant-v1",
        "page_size_tokens": 16,
        "n_layers": 0, "n_heads": 0, "head_dim": 0, "dtype": "bf16",
        "pages": [], "logical_seqs": [],
    });
    let cache_manifest = blobs.put(&serde_json::to_vec(&cache_envelope)?)?;

    let fingerprint = args.name.clone().unwrap_or_else(|| "pf-cli".into());
    let manifest = Manifest {
        schema_version: 1,
        media_type: MEDIATYPE_V1.to_owned(),
        agent: AgentInfo {
            kind: args.agent_id,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            fingerprint,
        },
        model: ModelLayer {
            base: model_base,
            diff: model_diff,
        },
        cache: CacheLayer {
            layout: "paged-batchinvariant-v1".into(),
            manifest: cache_manifest,
        },
        world: WorldLayer {
            fs: fs_digest,
            env: env_digest,
            procs: procs_digest,
        },
        effects: EffectsLayer {
            ledger: ledger_digest,
        },
        trace: TraceLayer {
            messages: trace_digest,
        },
        created_at: chrono::Utc::now(),
        // Parents are taken from --parent first; if absent and a
        // `.pfcid` sentinel exists at the fs-root (written by
        // `pf checkout`), use that. Closes the audit's "fork → edit
        // → snapshot → merge fails: no common ancestor" finding.
        parents: {
            let mut ps: Vec<pf_core::digest::Digest256> = args
                .parent
                .iter()
                .map(|s| pf_core::digest::Digest256::parse(s))
                .collect::<Result<_, _>>()
                .map_err(|e| CliError::BadInput(format!("--parent: {e}")))?;
            if ps.is_empty() {
                let sentinel = args.fs_root.join(".pfcid");
                if let Ok(contents) = std::fs::read_to_string(&sentinel) {
                    let trimmed = contents.trim();
                    if let Ok(d) = pf_core::digest::Digest256::parse(trimmed) {
                        ps.push(d);
                    }
                }
            }
            ps
        },
    };

    let cid = store.put_manifest(&manifest)?;
    println!("{cid}");
    Ok(())
}
