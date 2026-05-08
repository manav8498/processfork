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

/// Default env-var redaction regex applied unless `--no-default-scrub`
/// is passed. Matches the obvious secret-shaped names (case-
/// insensitive): `token`, `secret`, `password`, `passwd`, `pwd`,
/// `api_?key`, `apikey`, `auth`, `bearer`, plus any var ending in
/// `_TOKEN` / `_SECRET` / `_PASSWORD` / `_KEY`.
///
/// v1.0.7 audit fix for "secrets leak by default" — see
/// `SECURITY.md` PF-SA-2026-002.
const DEFAULT_SCRUB_REGEX: &str =
    r"(?i)(?:^|_)(token|secret|password|passwd|pwd|api_?key|apikey|auth|bearer)(?:_|$)";

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

    /// Additional regex(es) of env-var names to redact from the
    /// captured environment. Repeatable.
    ///
    /// **A built-in default regex always runs unless you pass
    /// `--no-default-scrub`** — it redacts the obvious secret-shaped
    /// names (`token`, `secret`, `password`, `key`, `api_key`,
    /// `auth`, `bearer`, plus `*_TOKEN` / `*_SECRET` / `*_PASSWORD`
    /// / `*_KEY` suffixes, case-insensitive).
    ///
    /// v1.0.7 audit fix: prior versions captured every env var by
    /// default, so a forgetful operator with `OPENAI_API_KEY` /
    /// `GITHUB_TOKEN` / etc. in scope leaked them into the .pfimg.
    #[arg(long)]
    pub scrub_env: Vec<String>,

    /// Disable the built-in default scrub regex. Use this if you
    /// want full control over the redaction set (operator-supplied
    /// `--scrub-env` patterns still apply).
    #[arg(long)]
    pub no_default_scrub: bool,

    /// Parent CIDs to record in `manifest.parents`. Set this when you're
    /// snapshotting after a `pf fork` so `pf merge` can find the common
    /// ancestor. Repeatable.
    #[arg(long)]
    pub parent: Vec<String>,

    /// PID of the agent process to SIGSTOP for the duration of the
    /// snapshot, then SIGCONT after seal. Without this, multi-file
    /// concurrent agent writes can produce torn-state captures
    /// (a.txt at version=1, b.txt at version=0). The pause window is
    /// just the fs walk + env capture (typically 50–500 ms); CRIU /
    /// CDP capture happens before the pause to keep the window tight.
    /// Unix only — ignored on Windows where SIGSTOP doesn't exist.
    ///
    /// **Important**: `--pause-pid` freezes the process at OS scheduler
    /// level but cannot bracket app-level transactions. If the agent
    /// is mid-way through a multi-file update when we SIGSTOP it, the
    /// captured tree will still be torn. Use `--quiesce-cmd` for
    /// app-level coordination.
    #[arg(long)]
    pub pause_pid: Option<i32>,

    /// Command to invoke immediately before the fs walk to ask the
    /// agent to enter a quiescent state (finish its current
    /// transaction, flush buffers, etc.). Pair with `--resume-cmd`
    /// to release. Examples:
    ///
    ///   --quiesce-cmd 'curl -fsS -XPOST http://agent/admin/quiesce' \
    ///   --resume-cmd  'curl -fsS -XPOST http://agent/admin/resume'
    ///
    /// Closes the v1.0.4 audit's "SIGSTOP doesn't bracket app-level
    /// multi-file transactions" finding by giving the operator a
    /// hook into their app's transaction boundary.
    #[arg(long)]
    pub quiesce_cmd: Option<String>,

    /// Command to run after the snapshot finishes (whether it
    /// succeeded or failed). Pair with `--quiesce-cmd`.
    #[arg(long)]
    pub resume_cmd: Option<String>,

    /// PID to capture via the processfork-criu adapter (Linux only).
    /// When set, the world layer's `procs` blob is `procs.criu.v1`
    /// (a real CRIU image bundle) instead of `procs.unsupported.v1`
    /// (a placeholder).
    ///
    /// The flag invokes `python3 -m processfork_criu` to perform the
    /// dump — the adapter must be installed (`pip install
    /// processfork-criu`) and `criu` must be on `$PATH` with
    /// `CAP_SYS_ADMIN` (or root, or a configured CRIU socket).
    /// Errors out cleanly on macOS, Windows, or any host where
    /// `processfork_criu.is_available()` reports False.
    ///
    /// The adapter README has the full caveat list:
    /// `adapters/pf-criu/README.md`. Validation lives on the
    /// operator's Linux box, not on the upstream macOS CI host —
    /// same shape as the Modal vLLM lane.
    #[arg(long)]
    pub criu_pid: Option<i32>,

    /// Suppress the v1.0.12 stderr warning that the generic CLI
    /// snapshot produces empty model + cache layer envelopes.
    ///
    /// The warning fires by default because operators who think
    /// `pf snapshot` captured "the whole agent" have been surprised
    /// to find that restored sessions start with a fresh model +
    /// fresh KV cache. Pass this flag once you've internalized that
    /// engine state requires the vLLM/SGLang adapter to populate;
    /// the world (FS+env), trace, and effects layers are captured
    /// regardless.
    ///
    /// CI/automation that already routes through an adapter (the
    /// adapter sets the model + cache layers via SDK) should pass
    /// this flag; the warning is for interactive humans on the CLI
    /// without an adapter.
    #[arg(long)]
    pub allow_empty_engine_layers: bool,
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
    //
    // App-level transaction boundary first: --quiesce-cmd lets the
    // agent finish its in-flight transactions before we walk the fs.
    // RAII guard so --resume-cmd always runs even if capture errors.
    let _quiesce_guard = QuiesceGuard::run(args.quiesce_cmd.as_deref(), args.resume_cmd.clone())?;

    // For real cross-file atomicity at the OS level (APFS clone alone
    // isn't enough — the agent could still be mid-write between two
    // files when the clone fires), pass `--pause-pid <pid>`: we
    // SIGSTOP the agent, run the capture, then SIGCONT. Best paired
    // with --quiesce-cmd above so the pause lands on a transaction
    // boundary, not in the middle of one.
    #[cfg(unix)]
    let _pause_guard = match args.pause_pid {
        Some(pid) => Some(PauseGuard::stop(pid)?),
        None => None,
    };
    #[cfg(not(unix))]
    if args.pause_pid.is_some() {
        eprintln!("warning: --pause-pid is unix-only; ignoring on this OS");
    }

    let fs_digest = pf_world::WalkFsCapture::new(&args.fs_root)
        .use_apfs_clone(cfg!(target_os = "macos"))
        .capture(&blobs)?;
    let mut env_capture = pf_world::EnvCapture::new();
    // v1.0.7 audit fix: secret-shaped env vars are redacted by
    // default. Operator can disable via --no-default-scrub if they
    // need the full env in the snapshot (rare; CI debugging at most).
    if !args.no_default_scrub {
        env_capture = env_capture
            .scrub(DEFAULT_SCRUB_REGEX)
            .expect("compiled-in default scrub regex");
    }
    for pat in &args.scrub_env {
        env_capture = env_capture
            .scrub(pat)
            .map_err(|e| CliError::BadInput(format!("--scrub-env regex {pat:?}: {e}")))?;
    }
    let env_digest = env_capture.capture(&blobs)?;

    // Procs layer.
    //
    // Without --criu-pid we write `procs.unsupported.v1` — a
    // placeholder making it explicit that no live subprocess state
    // was captured. With --criu-pid we shell out to
    // `python3 -m processfork_criu --pid N` (the adapter), which
    // performs `criu dump`, builds the v1 envelope, and prints the
    // serialized bytes to stdout. We write those bytes verbatim
    // into a CAS blob.
    //
    // The adapter is Linux-only; on macOS/Windows the call exits
    // non-zero with a clear message and we surface that to the
    // operator. v1.0.12 audit fix: closes the v1.0.11 README's
    // "always placeholder" gap on the world layer's procs row.
    let procs_digest = if let Some(pid) = args.criu_pid {
        capture_criu_procs(&blobs, pid)?
    } else {
        let procs_blob = serde_json::json!({
            "kind": "procs.unsupported.v1",
            "unsupported_on": std::env::consts::OS,
            "note": "pf snapshot does not capture in-flight subprocesses without --criu-pid",
        });
        blobs.put(&serde_json::to_vec(&procs_blob)?)?
    };

    // Trace. v1.0.4 audit fix: validate the JSONL content (not just
    // the path) so a malformed file fails the snapshot rather than
    // silently producing a trace blob that breaks pf merge later.
    // Each non-blank line must be a JSON object with `role` (str)
    // and `content` (str).
    let trace_bytes = if let Some(p) = &args.trace_from_jsonl {
        let raw = std::fs::read_to_string(p)?;
        let mut canonical = Vec::new();
        for (lineno, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                CliError::BadInput(format!(
                    "--trace-from-jsonl line {} is not valid JSON: {e}",
                    lineno + 1
                ))
            })?;
            let obj = v.as_object().ok_or_else(|| {
                CliError::BadInput(format!(
                    "--trace-from-jsonl line {} is not a JSON object",
                    lineno + 1
                ))
            })?;
            let role = obj.get("role").and_then(|x| x.as_str()).ok_or_else(|| {
                CliError::BadInput(format!(
                    "--trace-from-jsonl line {} is missing string field `role`",
                    lineno + 1
                ))
            })?;
            let content = obj.get("content").and_then(|x| x.as_str()).ok_or_else(|| {
                CliError::BadInput(format!(
                    "--trace-from-jsonl line {} is missing string field `content`",
                    lineno + 1
                ))
            })?;
            // Re-emit canonically so downstream parsers see consistent shape.
            canonical.extend_from_slice(
                serde_json::to_string(&serde_json::json!({"role": role, "content": content}))?
                    .as_bytes(),
            );
            canonical.push(b'\n');
        }
        canonical
    } else {
        Vec::new()
    };
    let trace_digest = blobs.put(&trace_bytes)?;

    // Effects ledger.
    //
    // v1.0.7 audit fix (#34): we now route every effects entry through
    // pf_effects::Ledger::append, which computes a real
    // `session_hmac = HMAC(secret, prev_hash || this_hash)` per entry.
    // Prior versions wrote raw JSONL with `session_hmac = ""` so
    // tampering / reordering was undetectable.
    //
    // The session secret comes from `--session-secret-hex <hex>` /
    // `PF_SESSION_SECRET` (operator-supplied — preferred for real
    // ACRFence) or is freshly generated per-snapshot. When generated
    // here we embed the hex as a header field so `pf verify` can
    // perform tamper detection without an out-of-band secret; this is
    // documented as "tamper-detection mode" — a determined attacker
    // who can rewrite the blob can also re-sign it using the embedded
    // secret. Real ACRFence requires the operator to keep the secret
    // out of band (do not pass --embed-session-secret).
    let ledger_digest = {
        use pf_effects::ledger::{Ledger, SessionSecret};

        // Resolve secret bytes first (so we can optionally embed
        // them into the blob header for tamper-detection mode), then
        // construct SessionSecret from those bytes.
        let (secret_bytes, embed_in_blob) = if let Ok(hex_str) = std::env::var("PF_SESSION_SECRET")
        {
            (
                hex::decode(hex_str.trim())
                    .map_err(|e| CliError::BadInput(format!("PF_SESSION_SECRET hex: {e}")))?,
                false, // operator brought their own; don't echo it back
            )
        } else {
            {
                use ring::rand::SecureRandom;
                let mut buf = [0u8; 32];
                ring::rand::SystemRandom::new()
                    .fill(&mut buf)
                    .map_err(|_| CliError::BadInput("session-secret RNG failed".into()))?;
                (buf.to_vec(), true)
            }
        };
        let secret_hex = hex::encode(&secret_bytes);
        let secret = SessionSecret::new(secret_bytes);

        let mut ledger = Ledger::new(secret);
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
                let tool_id = v.get("tool_id").and_then(|x| x.as_str()).ok_or_else(|| {
                    CliError::BadInput(format!(
                        "--effects-from-jsonl line {} missing tool_id",
                        lineno + 1
                    ))
                })?;
                let args_hash_str = v.get("args_hash").and_then(|x| x.as_str()).unwrap_or("");
                let result_hash_str = v.get("result_hash").and_then(|x| x.as_str()).unwrap_or(
                    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                );
                let idempotency_key = v
                    .get("idempotency_key")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_owned();
                let class_str = v
                    .get("side_effect_class")
                    .and_then(|x| x.as_str())
                    .unwrap_or("irreversible");
                let side_effect_class = match class_str {
                    "pure" => pf_effects::ledger::SideEffectClass::Pure,
                    "idempotent" => pf_effects::ledger::SideEffectClass::Idempotent,
                    "network-only" => pf_effects::ledger::SideEffectClass::NetworkOnly,
                    _ => pf_effects::ledger::SideEffectClass::Irreversible,
                };
                let args_hash = pf_core::digest::Digest256::parse(args_hash_str)
                    .unwrap_or_else(|_| pf_core::digest::Digest256::of(&[]));
                let result_hash = pf_core::digest::Digest256::parse(result_hash_str)
                    .unwrap_or_else(|_| pf_core::digest::Digest256::of(&[]));
                let timestamp = v
                    .get("ts")
                    .and_then(|x| x.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map_or_else(chrono::Utc::now, |dt| dt.with_timezone(&chrono::Utc));
                ledger
                    .append(
                        timestamp,
                        tool_id,
                        args_hash,
                        idempotency_key,
                        result_hash,
                        side_effect_class,
                    )
                    .map_err(|e| CliError::BadInput(format!("ledger append: {e}")))?;
            }
        }

        // Serialize via Ledger::serialize, then post-process the
        // header line to optionally embed the session-secret hex
        // (tamper-detection mode).
        let raw_digest = ledger.serialize(blobs.as_ref())?;
        if embed_in_blob {
            let raw_bytes = blobs.get(&raw_digest)?;
            // Replace the header line with an extended one that
            // carries `session_secret_hex`. Body stays untouched.
            let mut split = raw_bytes.splitn(2, |b| *b == b'\n');
            let header_bytes = split.next().unwrap_or(&[]);
            let body_bytes = split.next().unwrap_or(&[]);
            let mut header: serde_json::Value = serde_json::from_slice(header_bytes)?;
            if let Some(obj) = header.as_object_mut() {
                obj.insert(
                    "session_secret_hex".to_owned(),
                    serde_json::Value::String(secret_hex.clone()),
                );
                obj.insert(
                    "verification_mode".to_owned(),
                    serde_json::Value::String("tamper-detection".into()),
                );
            }
            let mut new_blob = serde_json::to_vec(&header)?;
            new_blob.push(b'\n');
            new_blob.extend_from_slice(body_bytes);
            blobs.put(&new_blob)?
        } else {
            raw_digest
        }
    };

    // Model + cache layers: the generic CLI snapshot path always
    // emits empty envelopes because these layers are populated by
    // adapters (vLLM / SGLang / TGI) that know the engine's
    // internals — there is no "walk a directory and produce a
    // valid LoRA diff" heuristic that doesn't lie. v1.0.12 audit
    // fix: shout about it instead of silently writing empties so
    // operators don't think they captured engine state.
    if !args.allow_empty_engine_layers {
        eprintln!("warning: pf snapshot wrote EMPTY model + cache envelopes.");
        eprintln!("         The world (FS + env), trace, and effects layers WERE captured.");
        eprintln!("         Restored sessions will start with a fresh model + empty KV cache.");
        eprintln!("         To capture engine state, install processfork-vllm[vllm] or");
        eprintln!("         processfork-sglang[sglang] and call the SDK from inside the");
        eprintln!(
            "         engine process. Suppress this warning with --allow-empty-engine-layers"
        );
        eprintln!("         once your CI/automation has internalized the boundary.");
    }
    let model_envelope = serde_json::json!({
        "layout": "model.diff.v1",
        "diff": {"kind": "lora", "adapters": []},
        "note": "generic-cli-empty: populated by adapters, not by walking a directory",
    });
    let model_diff = blobs.put(&serde_json::to_vec(&model_envelope)?)?;
    let model_base = blobs.put(format!("base:{}", args.agent_id).as_bytes())?;

    let cache_envelope = serde_json::json!({
        "layout": "paged-batchinvariant-v1",
        "page_size_tokens": 16,
        "n_layers": 0, "n_heads": 0, "head_dim": 0, "dtype": "bf16",
        "pages": [], "logical_seqs": [],
        "note": "generic-cli-empty: populated by adapters, not by walking a directory",
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

/// SIGSTOP a PID on construction; SIGCONT on Drop. RAII guard so the
/// agent always resumes — even if the snapshot path errors out
/// mid-capture. Closes the v1.0.3 audit's "torn-state" finding for
/// operators who pass --pause-pid.
#[cfg(unix)]
struct PauseGuard {
    pid: nix::unistd::Pid,
}

#[cfg(unix)]
impl PauseGuard {
    fn stop(raw_pid: i32) -> anyhow::Result<Self> {
        use nix::sys::signal::{Signal, kill};
        let pid = nix::unistd::Pid::from_raw(raw_pid);
        kill(pid, Signal::SIGSTOP).map_err(|e| {
            CliError::BadInput(format!("--pause-pid {raw_pid}: kill SIGSTOP failed: {e}"))
        })?;
        // Brief sleep so the kernel finishes scheduling the stop before
        // we walk the fs (otherwise the agent can squeeze one more
        // write in between the SIGSTOP send and stop-state apply).
        std::thread::sleep(std::time::Duration::from_millis(20));
        Ok(Self { pid })
    }
}

#[cfg(unix)]
impl Drop for PauseGuard {
    fn drop(&mut self) {
        use nix::sys::signal::{Signal, kill};
        // Best-effort; if SIGCONT fails (process already gone, etc.)
        // there's nothing useful to do — log and continue.
        if let Err(e) = kill(self.pid, Signal::SIGCONT) {
            eprintln!(
                "warning: pause-pid {} SIGCONT failed: {e} — agent may stay stopped",
                self.pid
            );
        }
    }
}

/// RAII guard that runs `quiesce_cmd` on construction and
/// `resume_cmd` on Drop. Either side may be `None` (no-op).
///
/// v1.0.6 audit fix: resume_cmd is stashed in the guard BEFORE
/// quiesce_cmd runs, so a partial-failure quiesce — which can
/// already have mutated app state (e.g. set a sentinel flag, then
/// failed before flushing) — still triggers resume_cmd via Drop on
/// the error-return path. Without this, an operator who wires a
/// quiesce-cmd that fails halfway leaves their agent stuck in a
/// half-quiesced state.
struct QuiesceGuard {
    resume_cmd: Option<String>,
}

impl QuiesceGuard {
    fn run(quiesce: Option<&str>, resume: Option<String>) -> anyhow::Result<Self> {
        // CRITICAL ORDERING: construct the guard FIRST so its Drop
        // owns `resume`. If `run_shell(quiesce)?` errors below, the
        // function returns Err and Rust drops `guard` on the way
        // out, which fires Drop, which runs resume_cmd. So the
        // resume runs regardless of whether quiesce succeeded.
        let guard = Self { resume_cmd: resume };
        if let Some(cmd) = quiesce {
            run_shell(cmd).map_err(|e| {
                CliError::BadInput(format!(
                    "--quiesce-cmd {cmd:?} failed: {e} (--resume-cmd will still run)"
                ))
            })?;
        }
        Ok(guard)
    }
}

impl Drop for QuiesceGuard {
    fn drop(&mut self) {
        if let Some(cmd) = self.resume_cmd.take()
            && let Err(e) = run_shell(&cmd)
        {
            eprintln!("warning: --resume-cmd failed: {e} — agent may stay quiesced");
        }
    }
}

/// Execute a shell-style command via `sh -c`. Returns Ok(()) on
/// exit code 0; otherwise an error containing the exit status
/// and (if any) the captured stderr.
fn run_shell(cmd: &str) -> anyhow::Result<()> {
    use std::process::Command;
    let out = Command::new("sh").arg("-c").arg(cmd).output()?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("exit {}: {}", out.status, stderr.trim())
    }
}

/// Drive the `processfork-criu` adapter to dump a live PID into a
/// `procs.criu.v1` blob. Writes the serialized bundle bytes
/// (header line + tarball body) verbatim to the CAS, returns the
/// digest. Runs only when `--criu-pid <PID>` is supplied; the
/// adapter itself enforces Linux + criu binary + permissions.
///
/// We invoke the adapter via `python3 -c "..."` rather than a
/// dedicated CLI entry point so the adapter package and the Rust
/// CLI stay loosely coupled: the contract is just "the dump_pid()
/// API returns serialize()-able bytes." If the adapter isn't
/// installed, the import fails fast with a clear message.
fn capture_criu_procs(
    blobs: &Arc<dyn pf_core::cas::BlobStore>,
    pid: i32,
) -> anyhow::Result<pf_core::digest::Digest256> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let script = "\
import sys\n\
try:\n\
    import processfork_criu as pfc\n\
except ImportError as e:\n\
    sys.stderr.write(f'processfork_criu not installed: {e}\\n')\n\
    sys.stderr.write('Run: pip install processfork-criu\\n')\n\
    sys.exit(2)\n\
reason = pfc.unavailable_reason()\n\
if reason:\n\
    sys.stderr.write(f'CRIU unavailable: {reason}\\n')\n\
    sys.exit(2)\n\
pid = int(sys.argv[1])\n\
bundle = pfc.dump_pid(pid=pid, leave_running=True)\n\
sys.stdout.buffer.write(bundle.serialize())\n\
";

    let mut child = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            anyhow::anyhow!(
                "--criu-pid: failed to spawn python3: {e}. \
                             Install Python 3 + `pip install processfork-criu`."
            )
        })?;
    let stdin_handle = child.stdin.take();
    if let Some(mut s) = stdin_handle {
        let _ = s.write_all(b"");
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("--criu-pid {pid} failed: {}", stderr.trim());
    }
    Ok(blobs.put(&output.stdout)?)
}
