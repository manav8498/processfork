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
    #[arg(long)]
    pub trace_from_jsonl: Option<PathBuf>,
}

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

    // World.
    let fs_digest = pf_world::WalkFsCapture::new(&args.fs_root).capture(&blobs)?;
    let env_blob = serde_json::json!({
        "kind": "env.v1",
        "cwd": args.fs_root.display().to_string(),
        "vars": std::env::vars().collect::<std::collections::BTreeMap<_,_>>(),
    });
    let env_digest = blobs.put(&serde_json::to_vec(&env_blob)?)?;
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

    // Effects (empty header-only ledger).
    let ledger_digest = blobs.put(b"{\"kind\":\"effects.ledger.v1\",\"entries\":0}\n")?;

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
        parents: vec![],
    };

    let cid = store.put_manifest(&manifest)?;
    println!("{cid}");
    Ok(())
}
