# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added — Phase 7 (SDKs)

- **Python SDK (`crates/pf-py/`)** — pyo3 0.22 bindings:
  - `processfork.PfStore.open(path)` — opens a store, `~` expanded.
  - `processfork.snapshot_filesystem(store, agent_kind, fs_root, env, messages)`
    captures all four layers + trace into a single manifest.
  - `processfork.checkout_filesystem(store, cid, target_path)` restores
    the world-layer FS tree atomically.
  - `processfork.read_manifest(store, cid)` returns the manifest as a
    Python `dict`.
  - `processfork.merge(store, a, b, alpha?, dare_p?, seed?)` runs the
    full Phase-6 engine; returns
    `{merged_cid, ancestor, overall, world_conflicts, trace_summary,
     model_applied_task_arithmetic}`.
  - `processfork.digest_of(bytes)` SHA-256 helper.
  - Hand-written type stubs at
    `crates/pf-py/python/processfork/_pf_py.pyi` + `py.typed` marker so
    `mypy --strict` callers get full hints.
  - `pyproject.toml` driving `maturin build --release --features
    extension-module`. Verified end-to-end: built a wheel, installed it
    into a fresh `uv venv` (Python 3.12), and ran 5 smoke tests
    (`crates/pf-py/python/tests/test_smoke.py`) — all pass.

- **TypeScript SDK (`crates/pf-ts/`)** — napi-rs 2.16 bindings:
  - `PfStore.open(path)` (factory), `physicalBytes()`.
  - `snapshotFilesystem`, `checkoutFilesystem`, `readManifest`, `merge`,
    `digestOf` — same surface as Python.
  - `MergeReport` / `WorldConflict` / `Message` / `MergeOpts` typed
    objects via `#[napi(object)]`.
  - Auto-generated `index.d.ts` + `index.js` from `napi build --release`.
  - Thin TS wrapper at `ts/index.ts` adds JSON-parsed `readManifest`
    and a typed `Manifest` interface.
  - `package.json` configured for napi triple-resolution across
    `x86_64-linux`, `aarch64-linux`, `aarch64-darwin`, `x86_64-darwin`.
  - `tsconfig.json` for the TS wrapper. Verified end-to-end: built
    `processfork.darwin-arm64.node` (1.8 MB) and ran 5 smoke tests
    (`crates/pf-ts/test/smoke.mjs`) via `node --test` — all pass.

### Added — Phase 6 (merge engine)

- `pf-merge::ancestor::find_lca`: BFS lowest-common-ancestor walk over
  the manifest parents DAG. Trivial cases (`a == b`, ancestor relations)
  short-circuit. Multi-parent (octopus) ancestors error explicitly with
  `AncestorError::OctopusUnsupported` per `agent_docs/merge-protocol.md`.
- `pf-merge::trace`: pluggable `Summarizer` trait + `StubSummarizer`
  test impl that deterministically concatenates B's last 4 divergent
  messages. `merge_trace(blobs, A, B, X, summarizer)` reads three
  trace blobs, summarizes B's divergence, and emits a new trace =
  `A.messages + [system: <summary>]`. Returns the new digest, the
  injected summary, and a char-÷-4 token-count estimate for the
  cache-layer re-prefill UX line. Live Anthropic API call gated
  behind the `live-summarizer` feature flag.
- `pf-merge::world::merge_world`: full three-way file diff on the
  `pf_world::FsTree` format, implementing the 9-row decision table
  from `agent_docs/merge-protocol.md` §"World" — including
  delete-vs-modify resolution, add-on-both-with-same-content as clean,
  and `<<<<<<< A / ======= / >>>>>>> B`-marker conflict blobs (real
  text blobs persisted to CAS, referenced from the merged tree).
  Returns `WorldMergeOutcome { merged_fs, conflicts, clean_paths }`.
  8 unit tests cover every row of the table.
- `pf-merge::effects::merge_effects`: emits an `effects.merged.v1`
  blob that references both parent ledgers (without forging a new
  HMAC chain over a re-signed merged ledger — that would either
  require sharing per-session secrets or breaking the chain).
  Pre-computes counts so `pf merge` UX can print "B's N
  irreversible calls cached as facts" without re-walking. Honours
  `replay_with_new_key` (the per-class `--replay-effects` overrides).
- `pf-merge::model::merge_model`: variant-dispatch wrapper around
  `pf_model::ties_merge` + `pf_model::dare`. LoRA merges by
  `(layer_id, matrix)`; Full merges by parameter name; IA³ merges by
  `(layer, matrix)`; InPlaceTtt is concatenated by step_id. Trivial
  cases (one or both empty) bypass task arithmetic. Kind mismatches
  (A is LoRA, B is Full) keep A and flag `kind_mismatch=true`.
- `pf-merge::engine::merge`: the top-level orchestrator. Auto-
  discovers the LCA (or accepts an `x_hint`), runs all four layer
  merges, assembles a new manifest with `parents = [a, b]`, and
  returns `MergeReport` with per-layer `MergeOutcome`
  (`Clean | Conflicted | Skipped`) plus the aggregated overall.
- 28 unit tests (5 ancestor + 4 trace + 8 world + 3 effects + 5
  model + 3 engine) + 3 integration tests
  (`tests/merge_round_trip.rs`) exercising the engine end-to-end on
  the synthetic fork-pair fixture from Phases 1–5.

### Aligned — Phase 1 fixture

- `pf_core::fixture::FixtureWorldCapture` now emits entries matching
  the canonical `pf_world::FsTreeEntry` schema (`mode`, `kind` fields
  added) so Phase-1 fixtures flow through Phase-6 merge cleanly.
- `pf_core::fixture::FixtureEffectsCapture` now prepends the
  `effects.ledger.v1` header line and includes `session_hmac` per
  ledger entry to satisfy the Phase-3 wire format.
- `pf_core::fixture::FixtureModelCapture` now wraps its synthetic
  random bytes in a `model.diff.v1` envelope (Full delta with one
  `synth_param` f32 vector) so Phase-5's `load_diff` can read it.

### Added — Phase 5 (model layer)

- `pf-model::diff::ModelDiff`: tagged enum (`kind: lora|ia3|full|in-place-ttt`)
  with one payload per kind:
  - `LoraDelta` → list of `LoraAdapter { layer_id, matrix, rank, in_dim,
    out_dim, a, b }` with dimension-validation on store. `canonicalize()`
    sorts adapters by `(layer_id, matrix)` for digest stability.
  - `IA3Delta` → `BTreeMap<layer_id_string, BTreeMap<matrix_name, scaling_vec>>`.
  - `FullDelta` → `BTreeMap<param_name, dense_delta>`.
  - `InPlaceTttDelta` → `Vec<TttStep>`, canonicalized by `step_id`.
- `pf-model::serialize::store_diff` / `load_diff`: validate-and-canonicalize
  + persist + restore through any `BlobStore` under wire format
  `model.diff.v1`. Layout-tag mismatch surfaces as `Error::Integrity`.
- `pf-model::merge::dare(delta, p, seed)`: drop fraction `p` of magnitudes,
  rescale survivors by `1/(1-p)`. SplitMix64-deterministic given `seed`.
- `pf-model::merge::ties_merge(deltas, params)`: TIES task arithmetic —
  trim bottom `keep_top` quantile by magnitude, sign-elect by majority
  magnitude, disjoint-merge same-sign survivors, scale by `alpha`. Default
  `α=0.5`, `keep_top=0.2` per `agent_docs/architecture.md` §4.4.
- 20 unit tests (DARE / TIES / trim / round-trip / canonicalize) + 4
  integration tests (`tests/model_round_trip.rs`):
  - every variant round-trips byte-identically through `FsBlobStore`
  - DARE→TIES composition stays bounded
  - CAS dedup on identical diffs
  - 64-case proptest sweep over random delta lengths, asserting
    `merged.len() == input.len()` and all entries finite.

### Added — Phase 4 (cache layer)

- `pf-cache::format`: `paged-batchinvariant-v1` wire format —
  `PageManifest`, `Page { ix, k, v }` (K and V content-addressed
  independently so a fork mutating only V shares its K page),
  `LogicalSeq { id, page_ixs, fill_in_last_page }`, `CacheMeta`
  (page_size_tokens, n_layers, n_heads, head_dim, dtype), `Dtype`
  (Bf16 / F16 / F32 / Fp8E4m3). `canonicalize()` sorts pages by ix and
  seqs by id so the manifest digest is invariant across iteration order.
- `pf-cache::pager::CachePager`: engine-agnostic interface every
  adapter implements — `pause`, `resume`, `occupied_pages`,
  `logical_seqs`, `read_page`, `allocate_pages`, `write_page`,
  `install_logical_seqs`.
- `pf-cache::pager::SyntheticCachePager`: in-process implementation
  used by every test; SplitMix64-deterministic page filler so identical
  seeds produce byte-identical pages (drives CAS dedup), different seeds
  diverge.
- `pf-cache::serialize::serialize_pages` / `deserialize_pages`:
  portable round-trip via the `BlobStore` trait — no GPU needed.
- `pf-cache::capture::capture_cache` / `restore_cache`: high-level
  one-shot helpers with pause/resume safety guard. Restore validates
  meta equality before touching the destination pager.
- Feature flags `vllm-adapter` and `sglang-adapter` (off by default)
  for the engine FFI shims that land in Phase 10.
- 16 unit tests + 4 integration tests
  (`tests/cache_round_trip.rs`):
  - byte-identical FS-blob-store round-trip
  - 12-fork CoW storage budget (≤ 1.5× one-fork) — Cache-layer
    proof of the §4.6 spec
  - logical-seq round-trip (id-canonicalized order)
  - 100-case proptest sweep over random page sets
- 1 GPU-gated skeleton test (`tests/cache_bit_exact_vllm.rs`) that
  `eprintln!`-skips off-GPU and is wired for the operator to enable
  with `PF_HAS_GPU=1` once `adapters/pf-vllm` lands in Phase 10.

### Added — Phase 3 (effects layer)

- `pf-effects::SideEffectClass`: `Pure | Idempotent | Irreversible | NetworkOnly`,
  declared by tool authors at registration time.
- `pf-effects::SessionSecret`: opaque HMAC-key wrapper with redacted `Debug`
  impl (never logs the secret); `::generate()` uses `ring::rand::SystemRandom`.
- `pf-effects::LedgerEntry` (`effects.entry.v1`): timestamp, tool_id,
  args_hash, idempotency_key, result_hash, side_effect_class, session_hmac.
  HMAC defined as `HMAC-SHA256(secret, prev_entry_hash || this_entry_minus_hmac)`.
- `pf-effects::Ledger`: append-only ledger with HMAC chaining, `verify()`
  scan, `serialize` / `deserialize` round-trip via `BlobStore`.
  Tampering with any entry breaks the chain at that index — defends against
  ACRFence semantic-rollback (arXiv 2603.20625).
- `pf-effects::ReplayPolicy`: per-class replay decisions (`InjectCachedResult`,
  `ReplayWithSameKey`, `ReplayWithNewKey`, `SurfaceAsFact`). Three presets:
  `default`, `strict`, `aggressive`. Default never re-issues `Irreversible`.
- `pf-effects::ToolProxy`: wraps a runtime's tool dispatch so every call
  hashes args, mints an idempotency key (ULID-shaped), runs the tool,
  hashes the result, and appends to the ledger atomically.
- `pf-effects::mint_idempotency_key()`: SHA-256(timestamp_ms ‖ 80 random bits).
  Tested for uniqueness over 256 consecutive calls.
- 14 unit tests + 4 conformance proptests (`tests/fuzz_replay.rs`) running
  1000 cases each, covering the four `agent_docs/effects-layer.md` invariants:
    1. Default policy never re-issues `Irreversible`.
    2. Idempotency keys are unique within a session.
    3. HMAC chain validates on untouched ledgers.
    4. Forking preserves no-duplicate-irreversible across siblings.

### Added — Phase 2 (world layer)

- `pf-world::WalkFsCapture`: portable rayon-parallel filesystem capture
  with deterministic per-tree digest, default ignore-list (`.git/objects`,
  `target`, `node_modules`), opt-in `use_apfs_clone` fast-path that
  `cp -c -R`-clones a directory in O(1) on macOS before walking, opt-in
  `follow_symlinks`, custom ignore fragments via builder API.
- `pf-world::restore_tree`: atomic rebuild of a captured tree —
  stages into a sibling temp dir, then `rename(2)` over `dst`. Refuses to
  overwrite an existing path.
- `pf-world::FsTree` / `FsTreeEntry` (`fs.tree.v1` wire format).
  Files / dirs / symlinks all round-trip; symlinks captured as symlinks
  (their targets recorded), not as the targets they happen to point at.
- `pf-world::EnvCapture`: serializes `std::env::vars()` + cwd into a
  sorted `BTreeMap` so the digest is deterministic across hosts.
  `.scrub("(?i)secret|token")`-style regex redaction; matching keys
  become `"<redacted>"` pre-seal.
- `pf-world::ProcsCapture`: tagged `procs.criu.v1` blob on Linux when
  the `criu` binary is in PATH (full dump+tar deferred to live-Linux
  CI gated by `$PF_HAS_CRIU=1`); `procs.unsupported.v1` placeholder
  with `unsupported_on: <os>` on every other host so restore can warn
  cleanly.
- 9 unit tests + 3 integration tests (`tests/world_round_trip.rs`):
  byte-identical FS round-trip on a 32 MiB / 256-file sandbox (or 1 GB
  if `PF_WORLD_TEST_GB=1`), env determinism, procs blob always emitted.

### Added — Phase 1 (core engine, Rust)

- `pf-core::cas::FsBlobStore`: on-disk content-addressed store, sharded by
  digest prefix, zstd-19 compressed, atomic write via temp+rename, on-read
  re-hash for corruption detection.
- `pf-core::cas::MemBlobStore`: in-memory variant for tests / `--ephemeral`.
- `pf-core::store::PfStore`: high-level wrapper bundling a `BlobStore` plus a
  manifest catalog (`images/<cid>.json` markers for fast `pf log`).
- `pf-core::snapshot::Snapshotter`: atomic four-layer snapshot orchestrator
  using `thread::scope` for concurrent capture; assembles + persists a v1
  `Manifest` in one call.
- `pf-core::fixture`: synthetic per-layer captures (model / cache / world /
  effects / trace) sized for the build host so the CI gate can run without a
  GPU.
- Integration test `tests/snapshot_synthetic_4layer.rs` asserting
  Phase-1 budgets: snapshot <500 ms, CAS dedup on identical content,
  12-fork storage ≤ 1.5× one-fork storage.
- `examples/01-hello-fork/`: end-to-end runnable example printing the
  snapshot CID, wall-clock time, and dedup delta.

Measured on the build host (macOS arm64): snapshot **8 ms** for the default
fixture (1.38 MB total payload), 60× headroom under the 500 ms budget;
identical second snapshot grows the store by **614 B** (the new manifest
JSON).

### Added — Phase 0 (bootstrap)

- Cargo workspace with 10 crates: `pf-core`, `pf-model`, `pf-cache`,
  `pf-world`, `pf-effects`, `pf-merge`, `pf-registry`, `pf-cli`, `pf-py`,
  `pf-ts`. All compile clean (`cargo check --workspace` — zero warnings).
- `pf-core::digest::Digest256` (SHA-256, OCI-style `sha256:<hex>`).
- `pf-core::manifest::Manifest` v1 schema with all four layer descriptors.
- `pf-core::cas::BlobStore` trait surface.
- `pf-core::error::Error` typed-error hierarchy.
- `pf` CLI scaffold rendering all 12 subcommands via `clap` derive.
- Agent infrastructure: `CLAUDE.md`, `agent_docs/*` (13 files),
  `.claude/agents/*` (5 sub-agents), `.claude/skills/*` (5 skills),
  `.claude/hooks/*` (3 hooks), `claude-progress.json`, `claude-plan.md`.
- Project meta: `LICENSE` (MIT), `README.md`, `SECURITY.md`,
  `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`, `.gitignore`.
