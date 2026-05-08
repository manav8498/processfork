// SPDX-License-Identifier: MIT
//! Python bindings for ProcessFork.
//!
//! Exposed surface (also documented in `python/processfork/_pf_py.pyi`):
//!
//! ```python
//! import processfork as pf
//! store = pf.PfStore.open("~/.processfork")
//! cid = pf.snapshot_filesystem(
//!     store, agent_kind="claude-code",
//!     fs_root="/tmp/sandbox",
//!     env={"PWD": "/tmp/sandbox"},
//!     messages=[{"role": "user", "content": "hi"}],
//! )
//! pf.checkout_filesystem(store, cid, target="/tmp/restored")
//! report = pf.merge(store, cid_a, cid_b)
//! ```
//!
//! Implementation strategy: the heavy lifting lives in the Rust crates.
//! This module just translates between Python types (str, dict, list) and
//! the Rust types they correspond to.

// pyo3 0.22's `#[pyfunction]` macro emits unsafe extraction calls that trip
// the edition-2024 `unsafe_op_in_unsafe_fn` lint. The macro audits the
// safety itself; we suppress for the crate. Re-evaluate when bumping
// pyo3 ≥0.23.
#![allow(unsafe_op_in_unsafe_fn)]
// Most pyo3 binding shims take owned values for FFI reasons, which clippy
// would otherwise flag. See `.claude/skills/pyo3-binding/SKILL.md`.
#![allow(clippy::needless_pass_by_value)]
// Public `#[pyclass]` / `#[pyfunction]` items are documented for Python
// users in the `.pyi` stubs and the docstrings inside `__init__.py`;
// asking rustdoc for redundant prose adds no value here.
#![allow(missing_docs)]
// `use pf_core::Error::*` is a deliberate ergonomic choice in the
// PyErr-mapping code; the variants are well-known.
#![allow(clippy::enum_glob_use)]
// pyo3's `?` propagation across PyErr conversions registers as
// "useless"; the conversions are required by the trait-resolver.
#![allow(clippy::useless_conversion)]
// `snapshot_filesystem` is one big sequenced layer-capture; splitting
// into helpers would obscure the reading order, not aid clarity.
// `collapsible_if` (new in Rust 1.88) fights nested `if let Some(...)
// = strip_prefix(...) { if let Ok(home) = var(...) }` which reads
// better as a staircase than a let-chain in this binding code.
#![allow(clippy::too_many_lines, clippy::collapsible_if)]

use std::path::PathBuf;
use std::sync::Arc;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::{
    AgentInfo, CacheLayer, EffectsLayer, MEDIATYPE_V1, Manifest, ModelLayer, TraceLayer, WorldLayer,
};
use pf_core::store::PfStore as RsPfStore;
use pf_merge::{MergeOptions, StubSummarizer};

// ----------------------- Error mapping -----------------------

fn map_err(e: pf_core::Error) -> PyErr {
    use pf_core::Error::*;
    match e {
        Io(io) => PyRuntimeError::new_err(format!("io: {io}")),
        Manifest(m) => PyValueError::new_err(format!("manifest: {m}")),
        Hex(h) => PyValueError::new_err(format!("hex: {h}")),
        InvalidDigest(s) => PyValueError::new_err(format!("invalid digest: {s}")),
        Integrity(s) => PyRuntimeError::new_err(format!("integrity: {s}")),
        Unimplemented(s) => PyRuntimeError::new_err(format!("not yet implemented: {s}")),
    }
}

fn map_merge_err(e: pf_merge::engine::MergeError) -> PyErr {
    PyRuntimeError::new_err(format!("merge: {e}"))
}

fn parse_cid(s: &str) -> PyResult<Digest256> {
    Digest256::parse(s).map_err(map_err)
}

// ----------------------- PfStore -----------------------

/// Wraps `pf_core::store::PfStore`. Cheap to clone; inner state is `Arc`'d.
#[pyclass(name = "PfStore")]
pub struct PyPfStore {
    inner: Arc<RsPfStore>,
}

#[pymethods]
impl PyPfStore {
    /// Open (or create) a ProcessFork store rooted at `path`.
    #[staticmethod]
    pub fn open(path: String) -> PyResult<Self> {
        let p = expand_user(&path);
        let s = RsPfStore::open(&p).map_err(map_err)?;
        Ok(Self { inner: Arc::new(s) })
    }

    /// Total physical bytes stored on disk (compressed).
    pub fn physical_bytes(&self) -> PyResult<u64> {
        self.inner.blobs().physical_bytes().map_err(map_err)
    }

    /// Sanity ping for the bindings; returns `"ok"`.
    pub fn __repr__(&self) -> String {
        format!("<PfStore at {}>", self.inner.root().display())
    }
}

/// `~` expansion without pulling in the `dirs` crate.
fn expand_user(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

// ----------------------- digest_of -----------------------

/// `pf.digest_of(bytes) -> str` — sanity helper used in tests and demos.
#[pyfunction]
pub fn digest_of(bytes: Vec<u8>) -> String {
    Digest256::of(&bytes).as_str().to_owned()
}

// ----------------------- snapshot_filesystem -----------------------

/// Built-in default env-var scrub regex. Mirrors the CLI's
/// `DEFAULT_SCRUB_REGEX` in `crates/pf-cli/src/commands/snapshot.rs`.
/// Applied to caller-supplied env unless `default_scrub_env=False`.
///
/// v1.0.9 audit fix: prior versions of the SDK stored the supplied env
/// dict verbatim, so adapters that called
/// `pf.snapshot_filesystem(..., env=dict(os.environ))` (every adapter
/// in `adapters/`) leaked `OPENAI_API_KEY`/`GITHUB_TOKEN`/etc. into the
/// blob even when the CLI path was already redacting them.
const DEFAULT_ENV_SCRUB: &str =
    r"(?i)(?:^|_)(token|secret|password|passwd|pwd|api_?key|apikey|auth|bearer)(?:_|$)";

/// Capture a filesystem sandbox + env + chat trace into a `.pfimg`-style
/// manifest. Returns the manifest CID (`sha256:...`).
///
/// `messages` is a list of `{"role": str, "content": str}` dicts.
///
/// `effects` (optional) is a list of tool-call ledger entries — each
/// a dict like
/// ``{"timestamp": ..., "tool_id": ..., "args_hash": ...,
///    "idempotency_key": ..., "result_hash": ...,
///    "side_effect_class": "pure"|"idempotent"|"irreversible"|"network-only"}``.
/// Adapters maintain this list as the agent runs; passing it here
/// folds it into the world image so restored agents see prior side
/// effects as facts (ACRFence — "won't double-send your email").
///
/// `default_scrub_env` (default `True`) applies the built-in
/// secret-shaped-name regex to `env` before storing. Adapters that
/// pass `dict(os.environ)` get safe-by-default redaction without
/// every caller having to remember it. Operators who want the full
/// env in the snapshot pass `default_scrub_env=False`.
///
/// `scrub_env` is an optional list of additional regex patterns; any
/// env-var name matching either the default or a custom pattern is
/// replaced with `"<redacted>"`.
#[pyfunction]
#[pyo3(signature = (
    store, agent_kind, fs_root, env, messages,
    effects = None, default_scrub_env = true, scrub_env = None,
    parents = None,
))]
// pyo3 binding shims map a Python kwargs surface 1:1 onto positional
// Rust arguments; splitting into a builder would obscure the
// signature without changing the FFI layout.
#[allow(clippy::too_many_arguments)]
pub fn snapshot_filesystem(
    store: &PyPfStore,
    agent_kind: String,
    fs_root: String,
    env: Bound<'_, PyDict>,
    messages: Bound<'_, PyList>,
    effects: Option<Bound<'_, PyList>>,
    default_scrub_env: bool,
    scrub_env: Option<Vec<String>>,
    parents: Option<Vec<String>>,
) -> PyResult<String> {
    let blobs: Arc<dyn BlobStore> = store.inner.blobs_arc();

    // World layer.
    let fs_root = expand_user(&fs_root);
    let walker = pf_world::WalkFsCapture::new(&fs_root);
    let fs_digest = walker.capture(&blobs).map_err(map_err)?;

    // Env: convert PyDict → BTreeMap → EnvSnapshot blob, applying the
    // default + caller-supplied scrub regexes so secret-shaped names
    // never reach the blob unredacted.
    let mut scrubs: Vec<regex::Regex> = Vec::new();
    if default_scrub_env {
        scrubs.push(
            regex::Regex::new(DEFAULT_ENV_SCRUB)
                .expect("compiled-in default env scrub regex must parse"),
        );
    }
    if let Some(extras) = scrub_env {
        for pat in extras {
            scrubs.push(
                regex::Regex::new(&pat)
                    .map_err(|e| PyValueError::new_err(format!("scrub_env regex {pat:?}: {e}")))?,
            );
        }
    }
    let mut env_map = std::collections::BTreeMap::new();
    for (k, v) in env.iter() {
        let key: String = k.extract()?;
        let val: String = v.extract()?;
        let redacted = scrubs.iter().any(|re| re.is_match(&key));
        env_map.insert(key, if redacted { "<redacted>".into() } else { val });
    }
    let env_blob = serde_json::json!({
        "kind": "env.v1",
        "cwd": fs_root.to_string_lossy().to_string(),
        "vars": env_map,
    });
    let env_digest = blobs
        .put(
            &serde_json::to_vec(&env_blob)
                .map_err(|e| PyValueError::new_err(format!("env serialize: {e}")))?,
        )
        .map_err(map_err)?;

    // Procs: portable placeholder.
    let procs_blob = serde_json::json!({
        "kind": "procs.unsupported.v1",
        "unsupported_on": std::env::consts::OS,
        "note": "pf-py does not yet capture in-flight subprocesses",
    });
    let procs_digest = blobs
        .put(
            &serde_json::to_vec(&procs_blob)
                .map_err(|e| PyValueError::new_err(format!("procs serialize: {e}")))?,
        )
        .map_err(map_err)?;

    // Trace.
    let mut trace_bytes = Vec::new();
    for msg in messages.iter() {
        let d: Bound<'_, PyDict> = msg.downcast_into()?;
        let role: String = d
            .get_item("role")?
            .ok_or_else(|| PyValueError::new_err("message missing 'role'"))?
            .extract()?;
        let content: String = d
            .get_item("content")?
            .ok_or_else(|| PyValueError::new_err("message missing 'content'"))?
            .extract()?;
        let entry = serde_json::json!({"role": role, "content": content});
        trace_bytes.extend_from_slice(
            &serde_json::to_vec(&entry)
                .map_err(|e| PyValueError::new_err(format!("trace serialize: {e}")))?,
        );
        trace_bytes.push(b'\n');
    }
    let trace_digest = blobs.put(&trace_bytes).map_err(map_err)?;

    // Effects ledger.
    //
    // v1.0.9 audit fix: prior versions of the SDK wrote raw JSONL
    // with `session_hmac = ""` so tamper / reorder / delete on the
    // on-disk blob was undetectable, even though the CLI path had
    // been hardened in v1.0.7. Now mirror the CLI: route every entry
    // through `pf_effects::ledger::Ledger::append`, which computes
    // `session_hmac = HMAC(secret, prev_hash || this_hash)`.
    //
    // Session secret comes from `PF_SESSION_SECRET` env var if
    // present (operator-supplied — preferred for real ACRFence), or
    // is freshly generated per snapshot. When generated here we
    // embed the hex into the blob header so `pf verify` can perform
    // tamper detection without an out-of-band secret. This is
    // "tamper-detection mode" — see CLI snapshot.rs for the same
    // reasoning.
    let ledger_digest = {
        use pf_effects::ledger::{Ledger, SessionSecret, SideEffectClass};

        let (secret_bytes, embed_in_blob) = if let Ok(hex_str) = std::env::var("PF_SESSION_SECRET")
        {
            (
                hex::decode(hex_str.trim())
                    .map_err(|e| PyValueError::new_err(format!("PF_SESSION_SECRET hex: {e}")))?,
                false,
            )
        } else {
            use ring::rand::SecureRandom;
            let mut buf = [0u8; 32];
            ring::rand::SystemRandom::new()
                .fill(&mut buf)
                .map_err(|_| PyRuntimeError::new_err("session-secret RNG failed"))?;
            (buf.to_vec(), true)
        };
        let secret_hex = hex::encode(&secret_bytes);
        let secret = SessionSecret::new(secret_bytes);

        let mut ledger = Ledger::new(secret);
        if let Some(list) = effects {
            for item in list.iter() {
                let d: Bound<'_, PyDict> = item.downcast_into()?;

                let get_str = |key: &str| -> PyResult<Option<String>> {
                    Ok(match d.get_item(key)? {
                        Some(v) => Some(v.extract::<String>()?),
                        None => None,
                    })
                };
                let tool_id = get_str("tool_id")?
                    .ok_or_else(|| PyValueError::new_err("effects entry missing 'tool_id'"))?;
                let args_hash_str = get_str("args_hash")?.unwrap_or_default();
                let result_hash_str = get_str("result_hash")?.unwrap_or_else(|| {
                    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                        .to_owned()
                });
                let idempotency_key = get_str("idempotency_key")?.unwrap_or_default();
                let class_str =
                    get_str("side_effect_class")?.unwrap_or_else(|| "irreversible".into());
                let side_effect_class = match class_str.as_str() {
                    "pure" => SideEffectClass::Pure,
                    "idempotent" => SideEffectClass::Idempotent,
                    "network-only" => SideEffectClass::NetworkOnly,
                    _ => SideEffectClass::Irreversible,
                };
                let args_hash =
                    Digest256::parse(&args_hash_str).unwrap_or_else(|_| Digest256::of(&[]));
                let result_hash =
                    Digest256::parse(&result_hash_str).unwrap_or_else(|_| Digest256::of(&[]));
                let timestamp = get_str("ts")?
                    .or(get_str("timestamp")?)
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map_or_else(chrono::Utc::now, |dt| dt.with_timezone(&chrono::Utc));

                ledger
                    .append(
                        timestamp,
                        &tool_id,
                        args_hash,
                        idempotency_key,
                        result_hash,
                        side_effect_class,
                    )
                    .map_err(|e| PyValueError::new_err(format!("ledger append: {e}")))?;
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
            let mut header: serde_json::Value = serde_json::from_slice(header_bytes)
                .map_err(|e| PyValueError::new_err(format!("ledger header: {e}")))?;
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
            let mut new_blob = serde_json::to_vec(&header)
                .map_err(|e| PyValueError::new_err(format!("ledger header re-encode: {e}")))?;
            new_blob.push(b'\n');
            new_blob.extend_from_slice(body_bytes);
            blobs.put(&new_blob).map_err(map_err)?
        } else {
            raw_digest
        }
    };

    // Model: empty Lora delta envelope (sufficient for v1; SDK users
    // attach real diffs through dedicated APIs once Phase-10 adapters
    // wire them in).
    let model_envelope = serde_json::json!({
        "layout": "model.diff.v1",
        "diff": {"kind": "lora", "adapters": []},
    });
    let model_diff = blobs
        .put(
            &serde_json::to_vec(&model_envelope)
                .map_err(|e| PyValueError::new_err(format!("model serialize: {e}")))?,
        )
        .map_err(map_err)?;
    let model_base = blobs
        .put(format!("base:{agent_kind}").as_bytes())
        .map_err(map_err)?;

    // Cache: empty page-manifest matching paged-batchinvariant-v1.
    let cache_envelope = serde_json::json!({
        "layout": "paged-batchinvariant-v1",
        "page_size_tokens": 16,
        "n_layers": 0,
        "n_heads": 0,
        "head_dim": 0,
        "dtype": "bf16",
        "pages": [],
        "logical_seqs": [],
    });
    let cache_manifest = blobs
        .put(
            &serde_json::to_vec(&cache_envelope)
                .map_err(|e| PyValueError::new_err(format!("cache serialize: {e}")))?,
        )
        .map_err(map_err)?;

    // Assemble manifest.
    let manifest = Manifest {
        schema_version: 1,
        media_type: MEDIATYPE_V1.to_owned(),
        agent: AgentInfo {
            kind: agent_kind,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            fingerprint: "pf-py".into(),
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
        // v1.0.13 audit fix: prior versions hard-coded `parents:
        // vec![]`, so SDK-only divergent snapshots could never be
        // 3-way-merged (no common ancestor was discoverable). The
        // CLI has had `--parent <CID>` since v1.0.3; the Python SDK
        // now exposes the same surface so adapters that fork via
        // the SDK can record lineage.
        parents: parents
            .unwrap_or_default()
            .into_iter()
            .map(|s| Digest256::parse(&s))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| PyValueError::new_err(format!("bad parent CID: {e}")))?,
    };
    let cid = store.inner.put_manifest(&manifest).map_err(map_err)?;
    Ok(cid.as_str().to_owned())
}

// ----------------------- checkout_filesystem -----------------------

/// Restore the world-layer FS tree of an image into `target_path`.
/// `target_path` must not already exist.
#[pyfunction]
pub fn checkout_filesystem(store: &PyPfStore, cid: String, target_path: String) -> PyResult<()> {
    let cid = parse_cid(&cid)?;
    let manifest = store.inner.get_manifest(&cid).map_err(map_err)?;
    let blobs = store.inner.blobs_arc();
    pf_world::restore_tree(&blobs, &manifest.world.fs, expand_user(&target_path)).map_err(map_err)
}

// ----------------------- read_manifest -----------------------

/// Load a manifest by CID and return it as a Python dict (decoded from
/// the canonical JSON wire format).
#[pyfunction]
pub fn read_manifest(py: Python<'_>, store: &PyPfStore, cid: String) -> PyResult<PyObject> {
    let cid = parse_cid(&cid)?;
    let m = store.inner.get_manifest(&cid).map_err(map_err)?;
    let json = serde_json::to_value(&m)
        .map_err(|e| PyValueError::new_err(format!("manifest serialize: {e}")))?;
    json_value_to_py(py, &json)
}

/// Fetch the raw bytes of a blob by digest. Returns a Python `bytes`.
/// Used by adapters that need to read individual layer blobs (e.g.
/// the LangGraph checkpointer reads the trace blob to reconstitute
/// state on `get()`).
#[pyfunction]
pub fn read_blob<'py>(
    py: Python<'py>,
    store: &PyPfStore,
    digest: String,
) -> PyResult<Bound<'py, pyo3::types::PyBytes>> {
    let d = parse_cid(&digest)?;
    let blobs: Arc<dyn BlobStore> = store.inner.blobs_arc();
    let bytes = blobs.get(&d).map_err(map_err)?;
    Ok(pyo3::types::PyBytes::new(py, &bytes))
}

/// Store `bytes` content-addressed; returns the resulting digest as
/// a `sha256:<hex>` string. Used by the vLLM/SGLang adapters to
/// persist KV-cache page bytes + their per-snapshot page manifest.
#[pyfunction]
pub fn put_blob(store: &PyPfStore, bytes: Vec<u8>) -> PyResult<String> {
    let blobs: Arc<dyn BlobStore> = store.inner.blobs_arc();
    let d = blobs.put(&bytes).map_err(map_err)?;
    Ok(d.as_str().to_owned())
}

fn json_value_to_py(py: Python<'_>, v: &serde_json::Value) -> PyResult<PyObject> {
    // pyo3 0.24: `IntoPy::into_py` is deprecated in favour of
    // `IntoPyObject::into_pyobject(...)?.unbind().into_any()`. The
    // `.into_pyobject(py)` returns a `Bound<'py, T>` (or its Result);
    // `.into_any()` widens to `Bound<'py, PyAny>`; `.unbind()` strips
    // the lifetime to give us an owned `Py<PyAny>` (= `PyObject`).
    use pyo3::IntoPyObject;
    use serde_json::Value;
    Ok(match v {
        Value::Null => py.None(),
        Value::Bool(b) => b.into_pyobject(py)?.to_owned().into_any().unbind(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_pyobject(py)?.into_any().unbind()
            } else if let Some(u) = n.as_u64() {
                u.into_pyobject(py)?.into_any().unbind()
            } else if let Some(f) = n.as_f64() {
                f.into_pyobject(py)?.into_any().unbind()
            } else {
                py.None()
            }
        }
        Value::String(s) => s.into_pyobject(py)?.into_any().unbind(),
        Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_value_to_py(py, item)?)?;
            }
            list.into_any().unbind()
        }
        Value::Object(obj) => {
            let dict = PyDict::new(py);
            for (k, val) in obj {
                dict.set_item(k, json_value_to_py(py, val)?)?;
            }
            dict.into_any().unbind()
        }
    })
}

// ----------------------- merge -----------------------

/// Three-way merge B into A, auto-discovering the common ancestor. Returns
/// `{"merged_cid": ..., "ancestor": ..., "overall": "clean"|"conflicted",
/// "world_conflicts": [...], "trace_summary": "..."}`.
#[pyfunction]
#[pyo3(signature = (store, a, b, alpha=None, dare_p=None, seed=None))]
pub fn merge(
    py: Python<'_>,
    store: &PyPfStore,
    a: String,
    b: String,
    alpha: Option<f32>,
    dare_p: Option<f64>,
    seed: Option<u64>,
) -> PyResult<PyObject> {
    let a = parse_cid(&a)?;
    let b = parse_cid(&b)?;
    let opts = MergeOptions {
        alpha: alpha.unwrap_or(0.5),
        dare_p: dare_p.unwrap_or(0.7),
        seed: seed.unwrap_or(0),
        replay_with_new_key: Vec::new(),
    };
    let report = pf_merge::merge(store.inner.as_ref(), &a, &b, &StubSummarizer, &opts, None)
        .map_err(map_merge_err)?;

    let dict = PyDict::new(py);
    dict.set_item("merged_cid", report.merged_cid.as_str())?;
    dict.set_item("ancestor", report.ancestor.as_str())?;
    dict.set_item(
        "overall",
        match report.overall {
            pf_merge::MergeOutcome::Clean => "clean",
            pf_merge::MergeOutcome::Conflicted => "conflicted",
            pf_merge::MergeOutcome::Skipped => "skipped",
        },
    )?;
    let conflicts = PyList::empty(py);
    for c in &report.world.conflicts {
        let cd = PyDict::new(py);
        cd.set_item("path", &c.path)?;
        cd.set_item("a_digest", c.a_digest.as_str())?;
        cd.set_item("b_digest", c.b_digest.as_str())?;
        cd.set_item("x_digest", c.x_digest.as_ref().map(Digest256::as_str))?;
        conflicts.append(cd)?;
    }
    dict.set_item("world_conflicts", conflicts)?;
    dict.set_item("trace_summary", &report.trace.injected_summary)?;
    dict.set_item(
        "model_applied_task_arithmetic",
        report.model.applied_task_arithmetic,
    )?;
    Ok(dict.into_any().unbind())
}

// ----------------------- module init -----------------------

#[pymodule]
fn _pf_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyPfStore>()?;
    m.add_function(wrap_pyfunction!(digest_of, m)?)?;
    m.add_function(wrap_pyfunction!(snapshot_filesystem, m)?)?;
    m.add_function(wrap_pyfunction!(checkout_filesystem, m)?)?;
    m.add_function(wrap_pyfunction!(read_manifest, m)?)?;
    m.add_function(wrap_pyfunction!(read_blob, m)?)?;
    m.add_function(wrap_pyfunction!(put_blob, m)?)?;
    m.add_function(wrap_pyfunction!(merge, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
