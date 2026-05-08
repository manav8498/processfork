// SPDX-License-Identifier: MIT
//! TypeScript bindings for ProcessFork via `napi-rs` 2.x.
//!
//! Mirrors the Python SDK surface in `crates/pf-py`. Auto-generates an
//! `index.d.ts` via `napi build`.

#![deny(clippy::all)]
#![allow(missing_docs)]
// doc'd in the generated index.d.ts + ts/index.ts
// napi-rs binding shims take owned values for FFI reasons.
// See `.claude/skills/napi-binding/SKILL.md`.
#![allow(clippy::needless_pass_by_value)]
// `snapshot_filesystem` is sequenced layer captures; splitting into
// helpers obscures the reading order, not clarity.
#![allow(clippy::too_many_lines)]
// `use std::fmt::Write` inside a function for the `write!` macro is
// idiomatic; clippy::items_after_statements doesn't help here.
#![allow(clippy::items_after_statements)]
// `collapsible_if` (new in Rust 1.88) fights nested staircase
// `if let Some(...) = strip_prefix(...) { if let Ok(home) = var(...) }`.
#![allow(clippy::collapsible_if)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use napi::{Error as NapiError, Status};
use napi_derive::napi;
use serde::{Deserialize, Serialize};

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::{
    AgentInfo, CacheLayer, EffectsLayer, MEDIATYPE_V1, Manifest, ModelLayer, TraceLayer, WorldLayer,
};
use pf_core::store::PfStore as RsPfStore;
use pf_merge::{MergeOptions, StubSummarizer};

fn map_err(e: pf_core::Error) -> NapiError {
    NapiError::new(Status::GenericFailure, format!("{e}"))
}
fn map_merge_err(e: pf_merge::engine::MergeError) -> NapiError {
    NapiError::new(Status::GenericFailure, format!("merge: {e}"))
}
fn parse_cid(s: &str) -> napi::Result<Digest256> {
    Digest256::parse(s).map_err(map_err)
}
fn expand_user(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

// ----------------------- digestOf -----------------------

/// `digestOf(bytes: Buffer): string` — SHA-256 of bytes, OCI-style.
#[napi]
#[allow(clippy::needless_pass_by_value)]
pub fn digest_of(bytes: Vec<u8>) -> String {
    Digest256::of(&bytes).as_str().to_owned()
}

// ----------------------- PfStore -----------------------

/// JS-side handle to a `pf_core::store::PfStore`. Cheap to clone (Arc'd).
#[napi]
pub struct PfStore {
    inner: Arc<RsPfStore>,
}

#[napi]
impl PfStore {
    /// Open (or create) a ProcessFork store at `path`. `~` is expanded.
    #[napi(factory)]
    pub fn open(path: String) -> napi::Result<Self> {
        let s = RsPfStore::open(expand_user(&path)).map_err(map_err)?;
        Ok(Self { inner: Arc::new(s) })
    }

    /// Total compressed bytes on disk.
    #[napi]
    pub fn physical_bytes(&self) -> napi::Result<u32> {
        let n = self.inner.blobs().physical_bytes().map_err(map_err)?;
        // napi 2.x doesn't expose u64 over the v8 boundary cleanly; we
        // clamp to u32 for the JS side. For stores ≥ 4 GiB call the
        // not-yet-exposed `physical_bytes_str()` (v1.1).
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }
}

// ----------------------- snapshotFilesystem -----------------------

/// Plain old JS object for one chat message.
#[napi(object)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// One ACRFence-shaped tool-call ledger entry. Mirrors the Python
/// SDK's `_EffectEntry` TypedDict — adapters maintain a list of these
/// as the agent runs, then pass the list via `opts.effects` so a
/// restored agent sees prior side effects as facts (ACRFence).
#[napi(object)]
#[derive(Default)]
pub struct EffectEntry {
    pub tool_id: String,
    pub args_hash: Option<String>,
    pub result_hash: Option<String>,
    pub idempotency_key: Option<String>,
    /// `"pure" | "idempotent" | "irreversible" | "network-only"`.
    /// Defaults to `"irreversible"` if absent.
    pub side_effect_class: Option<String>,
    /// RFC-3339 timestamp; defaults to `Utc::now()` when absent.
    pub timestamp: Option<String>,
}

/// Knobs for [`snapshot_filesystem`]. Optional — `None` keeps the safe
/// defaults: `defaultScrubEnv = true`, `effects = []`, `scrubEnv = []`.
#[napi(object)]
#[derive(Default)]
pub struct SnapshotOpts {
    /// Tool-call ledger entries; folded into the HMAC-chained
    /// `effects.ledger.v1` blob.
    pub effects: Option<Vec<EffectEntry>>,
    /// Default `true`. When `true` the built-in secret-shaped-name
    /// regex (matches `OPENAI_API_KEY`, `*_TOKEN`, `*_SECRET`,
    /// `*_PASSWORD`, `*_KEY`, `auth*`, `bearer*`, etc.) is applied to
    /// `env`. Set `false` only when you genuinely need the raw env in
    /// the snapshot.
    pub default_scrub_env: Option<bool>,
    /// Additional regex patterns; any env-var name matching either
    /// the default or a custom pattern becomes `"<redacted>"`.
    pub scrub_env: Option<Vec<String>>,
}

/// Built-in env-var scrub regex. Mirrors the CLI's `DEFAULT_SCRUB_REGEX`
/// in `crates/pf-cli/src/commands/snapshot.rs` and the SDK constant in
/// `crates/pf-py/src/lib.rs::DEFAULT_ENV_SCRUB`.
///
/// v1.0.10 audit fix: prior versions of the TS SDK stored the supplied
/// env object verbatim. JS callers that did
/// `snapshotFilesystem(..., { OPENAI_API_KEY: "..." })` leaked the raw
/// value into the blob, even though the CLI (v1.0.7) and Python SDK
/// (v1.0.9) both already redacted by default.
const DEFAULT_ENV_SCRUB: &str =
    r"(?i)(?:^|_)(token|secret|password|passwd|pwd|api_?key|apikey|auth|bearer)(?:_|$)";

/// `snapshotFilesystem(store, agentKind, fsRoot, env, messages, opts?): string`
///
/// Captures FS sandbox + env (with default redaction of secret-shaped
/// names) + chat trace + an HMAC-chained effects ledger into a `.pfimg`
/// manifest. Returns the manifest CID.
///
/// Set `opts.defaultScrubEnv = false` to disable env redaction, or
/// `opts.scrubEnv = [...]` to add custom regex patterns. Pass
/// `opts.effects` to fold tool-call ledger entries into the ACRFence
/// chain. Operators with `PF_SESSION_SECRET=<hex>` in env get real
/// out-of-band ACRFence; otherwise a fresh per-snapshot secret is
/// generated and embedded for tamper-detection mode.
#[napi]
#[allow(clippy::needless_pass_by_value)]
pub fn snapshot_filesystem(
    store: &PfStore,
    agent_kind: String,
    fs_root: String,
    env: BTreeMap<String, String>,
    messages: Vec<Message>,
    opts: Option<SnapshotOpts>,
) -> napi::Result<String> {
    let blobs: Arc<dyn BlobStore> = store.inner.blobs_arc();
    let opts = opts.unwrap_or_default();

    let fs_root_path = expand_user(&fs_root);
    let walker = pf_world::WalkFsCapture::new(&fs_root_path);
    let fs_digest = walker.capture(&blobs).map_err(map_err)?;

    // Env: apply default + caller-supplied scrub regexes BEFORE
    // serializing so secret-shaped names never reach the blob
    // unredacted. Mirrors the Python SDK fix in v1.0.9.
    let mut scrubs: Vec<regex::Regex> = Vec::new();
    if opts.default_scrub_env.unwrap_or(true) {
        scrubs.push(
            regex::Regex::new(DEFAULT_ENV_SCRUB)
                .expect("compiled-in default env scrub regex must parse"),
        );
    }
    if let Some(extras) = &opts.scrub_env {
        for pat in extras {
            scrubs.push(regex::Regex::new(pat).map_err(|e| {
                NapiError::new(Status::InvalidArg, format!("scrubEnv regex {pat:?}: {e}"))
            })?);
        }
    }
    let scrubbed_env: BTreeMap<String, String> = env
        .into_iter()
        .map(|(k, v)| {
            let redacted = scrubs.iter().any(|re| re.is_match(&k));
            (k, if redacted { "<redacted>".into() } else { v })
        })
        .collect();

    #[derive(Serialize)]
    struct EnvBlob<'a> {
        kind: &'a str,
        cwd: String,
        vars: &'a BTreeMap<String, String>,
    }
    let env_blob = EnvBlob {
        kind: "env.v1",
        cwd: fs_root_path.to_string_lossy().to_string(),
        vars: &scrubbed_env,
    };
    let env_digest =
        blobs
            .put(&serde_json::to_vec(&env_blob).map_err(|e| {
                NapiError::new(Status::GenericFailure, format!("env serialize: {e}"))
            })?)
            .map_err(map_err)?;

    #[derive(Serialize)]
    struct ProcsBlob<'a> {
        kind: &'a str,
        unsupported_on: &'a str,
        note: &'a str,
    }
    let procs_blob = ProcsBlob {
        kind: "procs.unsupported.v1",
        unsupported_on: std::env::consts::OS,
        note: "pf-ts does not yet capture in-flight subprocesses",
    };
    let procs_digest =
        blobs
            .put(&serde_json::to_vec(&procs_blob).map_err(|e| {
                NapiError::new(Status::GenericFailure, format!("procs serialize: {e}"))
            })?)
            .map_err(map_err)?;

    #[derive(Serialize)]
    struct TraceMsg<'a> {
        role: &'a str,
        content: &'a str,
    }
    let mut trace_bytes = Vec::new();
    for m in &messages {
        let entry = TraceMsg {
            role: m.role.as_str(),
            content: m.content.as_str(),
        };
        trace_bytes.extend_from_slice(&serde_json::to_vec(&entry).map_err(|e| {
            NapiError::new(Status::GenericFailure, format!("trace serialize: {e}"))
        })?);
        trace_bytes.push(b'\n');
    }
    let trace_digest = blobs.put(&trace_bytes).map_err(map_err)?;

    // Effects ledger.
    //
    // v1.0.10 audit fix: prior versions of the TS SDK always wrote an
    // empty raw blob (`{"kind":"effects.ledger.v1","entries":0}\n`),
    // so TS integrations had no ACRFence "won't double-send" guard,
    // even when callers supplied tool-call entries. Now route every
    // entry through `pf_effects::ledger::Ledger::append`, computing
    // per-entry `session_hmac = HMAC(secret, prev_hash || this_hash)`
    // — same code path as the Python SDK (v1.0.9) and CLI (v1.0.7).
    //
    // Session secret comes from `PF_SESSION_SECRET` env var if
    // present (operator-supplied — preferred for real ACRFence), or
    // is freshly generated per snapshot. When generated here we
    // embed the hex into the blob header so `pf verify` can perform
    // tamper detection without an out-of-band secret.
    let ledger_digest = {
        use pf_effects::ledger::{Ledger, SessionSecret, SideEffectClass};

        let (secret_bytes, embed_in_blob) = if let Ok(hex_str) = std::env::var("PF_SESSION_SECRET")
        {
            (
                hex::decode(hex_str.trim()).map_err(|e| {
                    NapiError::new(Status::InvalidArg, format!("PF_SESSION_SECRET hex: {e}"))
                })?,
                false,
            )
        } else {
            use ring::rand::SecureRandom;
            let mut buf = [0u8; 32];
            ring::rand::SystemRandom::new()
                .fill(&mut buf)
                .map_err(|_| NapiError::new(Status::GenericFailure, "session-secret RNG failed"))?;
            (buf.to_vec(), true)
        };
        let secret_hex = hex::encode(&secret_bytes);
        let secret = SessionSecret::new(secret_bytes);

        let mut ledger = Ledger::new(secret);
        if let Some(entries) = opts.effects {
            for e in entries {
                let class_str = e.side_effect_class.unwrap_or_else(|| "irreversible".into());
                let side_effect_class = match class_str.as_str() {
                    "pure" => SideEffectClass::Pure,
                    "idempotent" => SideEffectClass::Idempotent,
                    "network-only" => SideEffectClass::NetworkOnly,
                    _ => SideEffectClass::Irreversible,
                };
                let args_hash = e
                    .args_hash
                    .as_deref()
                    .and_then(|s| Digest256::parse(s).ok())
                    .unwrap_or_else(|| Digest256::of(&[]));
                let result_hash = e
                    .result_hash
                    .as_deref()
                    .and_then(|s| Digest256::parse(s).ok())
                    .unwrap_or_else(|| Digest256::of(&[]));
                let timestamp = e
                    .timestamp
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map_or_else(chrono::Utc::now, |dt| dt.with_timezone(&chrono::Utc));
                ledger
                    .append(
                        timestamp,
                        &e.tool_id,
                        args_hash,
                        e.idempotency_key.unwrap_or_default(),
                        result_hash,
                        side_effect_class,
                    )
                    .map_err(|e| {
                        NapiError::new(Status::GenericFailure, format!("ledger append: {e}"))
                    })?;
            }
        }

        let raw_digest = ledger.serialize(blobs.as_ref()).map_err(map_err)?;
        if embed_in_blob {
            // Replace the header line with an extended one that
            // carries `session_secret_hex` so `pf verify` can run
            // without an out-of-band secret. Body stays untouched.
            let raw_bytes = blobs.get(&raw_digest).map_err(map_err)?;
            let mut split = raw_bytes.splitn(2, |b| *b == b'\n');
            let header_bytes = split.next().unwrap_or(&[]);
            let body_bytes = split.next().unwrap_or(&[]);
            let mut header: serde_json::Value =
                serde_json::from_slice(header_bytes).map_err(|e| {
                    NapiError::new(Status::GenericFailure, format!("ledger header: {e}"))
                })?;
            if let Some(obj) = header.as_object_mut() {
                obj.insert(
                    "session_secret_hex".to_owned(),
                    serde_json::Value::String(secret_hex),
                );
                obj.insert(
                    "verification_mode".to_owned(),
                    serde_json::Value::String("tamper-detection".into()),
                );
            }
            let mut new_blob = serde_json::to_vec(&header).map_err(|e| {
                NapiError::new(
                    Status::GenericFailure,
                    format!("ledger header re-encode: {e}"),
                )
            })?;
            new_blob.push(b'\n');
            new_blob.extend_from_slice(body_bytes);
            blobs.put(&new_blob).map_err(map_err)?
        } else {
            raw_digest
        }
    };

    let model_envelope = serde_json::json!({
        "layout": "model.diff.v1",
        "diff": {"kind": "lora", "adapters": []},
    });
    let model_diff =
        blobs
            .put(&serde_json::to_vec(&model_envelope).map_err(|e| {
                NapiError::new(Status::GenericFailure, format!("model serialize: {e}"))
            })?)
            .map_err(map_err)?;
    let model_base = blobs
        .put(format!("base:{agent_kind}").as_bytes())
        .map_err(map_err)?;

    let cache_envelope = serde_json::json!({
        "layout": "paged-batchinvariant-v1",
        "page_size_tokens": 16,
        "n_layers": 0, "n_heads": 0, "head_dim": 0, "dtype": "bf16",
        "pages": [], "logical_seqs": []
    });
    let cache_manifest =
        blobs
            .put(&serde_json::to_vec(&cache_envelope).map_err(|e| {
                NapiError::new(Status::GenericFailure, format!("cache serialize: {e}"))
            })?)
            .map_err(map_err)?;

    let manifest = Manifest {
        schema_version: 1,
        media_type: MEDIATYPE_V1.to_owned(),
        agent: AgentInfo {
            kind: agent_kind,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            fingerprint: "pf-ts".into(),
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
    Ok(store
        .inner
        .put_manifest(&manifest)
        .map_err(map_err)?
        .as_str()
        .to_owned())
}

// ----------------------- checkoutFilesystem -----------------------

/// `checkoutFilesystem(store, cid, targetPath): void`. `targetPath` MUST NOT
/// already exist.
#[napi]
pub fn checkout_filesystem(store: &PfStore, cid: String, target_path: String) -> napi::Result<()> {
    let cid = parse_cid(&cid)?;
    let manifest = store.inner.get_manifest(&cid).map_err(map_err)?;
    let blobs = store.inner.blobs_arc();
    pf_world::restore_tree(&blobs, &manifest.world.fs, expand_user(&target_path)).map_err(map_err)
}

// ----------------------- readManifest -----------------------

/// `readManifest(store, cid): string` — returns the manifest JSON as a
/// string. The TS wrapper at `ts/index.ts` JSON-parses for callers that
/// want a typed object.
#[napi]
pub fn read_manifest(store: &PfStore, cid: String) -> napi::Result<String> {
    let cid = parse_cid(&cid)?;
    let m = store.inner.get_manifest(&cid).map_err(map_err)?;
    serde_json::to_string(&m)
        .map_err(|e| NapiError::new(Status::GenericFailure, format!("serialize: {e}")))
}

// ----------------------- readBlob -----------------------

/// `readBlob(store, digest): Buffer` — fetch raw bytes of a content-
/// addressed blob. Mirrors the Python SDK's `processfork.read_blob`.
/// Adapters that need to inspect individual layer blobs
/// (`world.env`, `effects.ledger`, etc.) call this; otherwise prefer
/// `readManifest` for the high-level view.
#[napi]
pub fn read_blob(store: &PfStore, digest: String) -> napi::Result<Vec<u8>> {
    let d = parse_cid(&digest)?;
    store.inner.blobs().get(&d).map_err(map_err)
}

// ----------------------- merge -----------------------

#[napi(object)]
pub struct WorldConflict {
    pub path: String,
    pub a_digest: String,
    pub b_digest: String,
    pub x_digest: Option<String>,
}

#[napi(object)]
pub struct MergeReport {
    pub merged_cid: String,
    pub ancestor: String,
    pub overall: String, // "clean" | "conflicted" | "skipped"
    pub world_conflicts: Vec<WorldConflict>,
    pub trace_summary: String,
    pub model_applied_task_arithmetic: bool,
}

#[napi(object)]
#[derive(Default, Deserialize)]
pub struct MergeOpts {
    pub alpha: Option<f64>,
    pub dare_p: Option<f64>,
    pub seed: Option<u32>,
}

/// `merge(store, a, b, opts?): MergeReport` — three-way merge B into A.
#[napi]
#[allow(clippy::needless_pass_by_value, clippy::cast_possible_truncation)]
pub fn merge(
    store: &PfStore,
    a: String,
    b: String,
    opts: Option<MergeOpts>,
) -> napi::Result<MergeReport> {
    let opts = opts.unwrap_or_default();
    let a = parse_cid(&a)?;
    let b = parse_cid(&b)?;
    let merge_opts = MergeOptions {
        alpha: opts.alpha.unwrap_or(0.5) as f32,
        dare_p: opts.dare_p.unwrap_or(0.7),
        seed: u64::from(opts.seed.unwrap_or(0)),
        replay_with_new_key: Vec::new(),
    };
    let report = pf_merge::merge(
        store.inner.as_ref(),
        &a,
        &b,
        &StubSummarizer,
        &merge_opts,
        None,
    )
    .map_err(map_merge_err)?;

    Ok(MergeReport {
        merged_cid: report.merged_cid.as_str().to_owned(),
        ancestor: report.ancestor.as_str().to_owned(),
        overall: match report.overall {
            pf_merge::MergeOutcome::Clean => "clean".into(),
            pf_merge::MergeOutcome::Conflicted => "conflicted".into(),
            pf_merge::MergeOutcome::Skipped => "skipped".into(),
        },
        world_conflicts: report
            .world
            .conflicts
            .iter()
            .map(|c| WorldConflict {
                path: c.path.clone(),
                a_digest: c.a_digest.as_str().to_owned(),
                b_digest: c.b_digest.as_str().to_owned(),
                x_digest: c.x_digest.as_ref().map(|d| d.as_str().to_owned()),
            })
            .collect(),
        trace_summary: report.trace.injected_summary.clone(),
        model_applied_task_arithmetic: report.model.applied_task_arithmetic,
    })
}
