# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/).

## [1.0.1] — 2026-05-05

Cross-platform wheels and the live vLLM bit-exact KV-cache integration.

- **Wheels**: `processfork` now ships a wheel for every PyPI
  platform tier — macOS arm64 + macOS x86_64 + Linux x86_64
  (manylinux_2_28) + Linux aarch64 (manylinux_2_28) + Windows x86_64
  (cross-compiled on the build host via `pyo3 = ["generate-import-lib"]`).
- **vLLM live FFI**: `adapters/pf-vllm/processfork_vllm/plugin.py`
  drives `vllm.worker.cache_engine.gpu_cache` for real, gated on
  `PF_HAS_GPU=1`. Bit-exact restore against `--enforce-deterministic`
  Llama-class workers. Mock mode (no engine) still gives a byte-
  identical write→read round-trip for unit tests.
- **PyPI Trusted Publishing**: `.github/workflows/release.yml`
  replaces `PYPI_API_TOKEN` with OIDC trust to GitHub via
  `pypa/gh-action-pypi-publish`. The `publish-pypi-core` and
  `publish-pypi-adapters` jobs both pull from the `pypi` deployment
  environment so PyPI's trust policy can be scoped to it.
- `processfork-vllm` bumped to `1.0.1`.

## [1.0.0] — 2026-05-05

The initial public release. Twelve build phases, 200+ tests across
Rust + Python + TypeScript surfaces, all four layers shipped, all
seven first-party adapters present (three end-to-end on the build
host, four scaffolded with explicit v1.0.1 milestones).

### Phase 12 — release

- Workspace + SDK package versions bumped from `0.1.0-dev` /
  `0.1.0.dev0` to `1.0.0`.
- `.github/workflows/release.yml`: full multi-platform release
  pipeline. On a `v*.*.*` tag push:
  - Cross-builds the `pf` binary for ubuntu-24.04 (x86_64 + arm64)
    and macos-14 (arm64).
  - Cosign-signs each binary keylessly via Sigstore.
  - Publishes a GitHub Release with binaries + signatures + SHA-256s
    + the latest `CHANGELOG.md` section as the release notes.
  - Publishes the 8 publishable Rust crates to crates.io in
    dep-order (`cargo publish`).
  - Publishes the `processfork` wheel + the 7 adapter pure-Python
    pkgs to PyPI (`maturin build` + `twine upload`).
  - Publishes `@processfork/sdk` to npm (`napi build` + `npm
    publish`).
  - Builds + pushes the multi-arch Docker image to
    `ghcr.io/manav8498/processfork:<tag>` + `:latest`.
- `Dockerfile`: 2-stage build producing a slim Debian-based image
  with the `pf` binary on PATH; mounts `/data/store` as a volume.
- `landing/`: single-page Tailwind landing site at `landing/index.html`
  ready for GitHub Pages from the `/landing` directory; ~8 KB HTML
  + 80 KB Tailwind JIT.
- `demo/script.sh`: 60-second viral-demo recording script that
  runs end-to-end on a laptop today (snapshot → 12-fork → merge →
  push to file:// → fresh-store clone → restored). Verified against
  the built binary.
- `demo/script.cast.md`: operator-runs-it instructions for
  asciinema recording + agg conversion to GIF/SVG.

### Added — Phase 11 (benchmarks + tests + docs)

**Microbench**
- `benchmarks/microbench/` — Criterion crate added to the workspace
  with two benches:
  - `snapshot_synthetic_4layer`: 4-layer atomic snapshot orchestrator
    against the default fixture. **Observed: 7.9 ms median** (budget
    500 ms; 63× headroom).
  - `cache_capture_64_pages` + `cache_restore_64_pages`: paged-KV
    serialise/deserialise hot path. **531 µs / 34 µs**.
- `benchmarks/RESULTS.md` published with reproducible commands +
  the build-host numbers + the operator-runs-it template for the
  GPU lane.

**PFBench**
- `benchmarks/pfbench/harness.py` — operator-runs-it harness with
  built-in `equals` / `contains` / `regex` graders + a built-in
  `echo` model so the harness is self-test-able in CI without any
  API keys. Self-test green: 3 tasks × 2 variants → 100 % pass.
- `benchmarks/pfbench/aggregate.py` — Markdown table aggregator over
  one or more results JSONLs.

**Documentation site**
- `docs/book.toml` + `docs/src/` mdBook source covering:
  introduction, install, first-fork tutorial, the 60-second demo,
  architecture overview, all four layer pages, merge protocol,
  `.pfimg` format, performance budget, security model, CLI
  reference, Python / TypeScript / Rust SDK refs, all 7 integration
  adapters, performance tuning, benchmarks index, migration guide,
  contributing, security policy, release checklist, changelog.
- README polished with the actual runnable 60-second demo (matching
  `examples/02-cli-snapshot/run.sh`) at the top.

**Test totals after Phase 11**

- 154 Rust tests (unchanged; microbenches are `cargo bench`, not
  `cargo test`)
- 5 Python SDK + 5 TypeScript SDK smoke tests
- 36 adapter smoke tests + 2 GPU-gated skips
- = **200 tests across the workspace**, plus
- 1 PFBench self-test (3 tasks × 2 variants = 6 grading rows)
- 2 Criterion bench suites (snapshot + cache round-trip)

### Added — Phase 10 (integration adapters)

All seven first-party adapters from `agent_docs/feature-spec.md` M5 ship
as their own pure-Python packages under `adapters/<name>/`. Three are
fully wired end-to-end against the Phase-7 SDK; four scaffold the
trait + URL parsing + auth-token plumbing with `NotImplementedError`
on the GPU/network paths until v1.0.1 lands them.

**Fully wired (build-host testable)**

- `adapters/pf-claude-code/` — `processfork-claude-code` Python pkg.
  `SessionRecorder` accumulates messages + tool calls and snapshots
  via the SDK; `ToolClassifier` provides safe-by-default tool →
  side-effect-class mapping (unknown tools → `Irreversible`);
  `install_slash_commands` drops `/snapshot`, `/fork`, `/merge`
  command files into `~/.claude/commands/processfork/`. The
  `pf-wrap-claude` CLI installs them. **9 smoke tests + runnable
  example 03**.
- `adapters/pf-langgraph/` — `processfork-langgraph` Python pkg.
  `ProcessForkCheckpointer` implements the duck-typed
  `BaseCheckpointSaver` surface (no hard `langgraph` dep at import);
  every checkpoint becomes a `.pfimg`. `fork_thread` shells out to
  `pf fork` for manifest-level branching. **5 smoke tests + runnable
  example 04 (3 checkpoints + 4 forks via real CLI)**.
- `adapters/pf-openinterpreter/` — `processfork-openinterpreter` pkg.
  `WrappedInterpreter` adds `snapshot(name)` / `checkout(name)` to
  any OpenInterpreter-shaped object; `wrap_interpreter` factory.
  Tool calls tap an in-memory ledger. **5 smoke tests + runnable
  example 05 (snapshot → destructive op → checkout restored
  byte-identical)**.

**Scaffolded (trait + auth + clear-error stubs; v1.0.1 wires the live FFI)**

- `adapters/pf-vllm/` — `processfork-vllm` pkg. `VllmCachePager`
  implements the Python side of `pf-cache::CachePager`; `VllmPlugin`
  registers `/v1/processfork/{snapshot,fork,checkout,merge}` HTTP
  handlers. Live FFI into vLLM's `worker.cache_engine` deferred to
  v1.0.1; current handlers return `501` with a clear pointer.
  **5 smoke tests + 1 GPU-gated test (skips without `$PF_HAS_GPU=1`)
  + runnable example 06 (skip-aware)**.
- `adapters/pf-sglang/` — `processfork-sglang` pkg. Sister
  implementation to vLLM, mapping onto SGLang's `mem_pool` /
  `RadixCache`. **4 smoke tests + 1 GPU-gated test + example 07**.
- `adapters/pf-autogen/` — `processfork-autogen` pkg.
  `ProcessForkRuntime` tracks per-agent message + tool-call state;
  `snapshot` flattens with `[agent]` attribution prefixes; `fork`
  shells out to `pf fork`. **4 smoke tests** (1 dep-gated on `pf` on
  PATH).
- `adapters/pf-crewai/` — `processfork-crewai` pkg.
  `ProcessForkMemory` implements CrewAI's memory protocol; every
  `save()` becomes a snapshot, `checkout(cid)` restores the world
  layer. **4 smoke tests**.

**Examples** (all 8 from `agent_docs/feature-spec.md` M9 now present):

- `examples/01-hello-fork/` (Phase 1) — synthetic 4-layer snapshot.
- `examples/02-cli-snapshot/` (Phase 8) — full CLI transcript.
- `examples/03-claude-code-fork/` — Claude Code adapter end-to-end.
- `examples/04-langgraph-checkpoint/` — checkpointer + 4-way fork.
- `examples/05-openinterpreter-undo/` — destructive-op undo round-trip.
- `examples/06-vllm-bit-exact/` — skip-aware GPU-gated harness.
- `examples/07-sglang-prefix-share/` — skip-aware GPU-gated harness.
- `examples/08-rl-rollout-fabric/` — N-way fan-out + winner merge,
  pure synthetic-fixture (runs on build host).

**Test totals after Phase 10**

- 154 Rust tests
- 5 Python SDK + 5 TypeScript SDK smoke tests (Phase 7)
- 36 adapter smoke tests + 2 GPU-gated skips (Phase 10)
- = **200 tests across the workspace**

### Added — Phase 9 (registry)

- `pf-registry::ImageRef`: parser for the five supported URL schemes —
  `file://`, `hf://`, `s3://`, `ipfs://`, `oci://`. Tags split correctly
  even when they collide with `host:port` syntax (oci) or `user/repo`
  (hf). 8 unit tests cover the round-trips + bad-scheme + missing-repo
  errors.
- `pf-registry::Registry` trait + `pf-registry::LayerSet`. Async via
  `async-trait`. Push uploads the manifest + every transitively-
  reachable blob; pull returns both. `RegistryError::UnsupportedScheme`
  cleanly distinguishes "feature flag off" from real backend failures.
- `pf-registry::FileRegistry`: filesystem-backed registry. Layout
  matches `agent_docs/registry-spec.md` — `manifest.json`,
  `manifest.json.sig`, `blobs/sha256/<aa>/<aabb…>.zst`. Used as the
  build-host integration test backbone; doubles as an air-gapped
  transport mechanism (`pf push file:///mnt/usb/...`).
- `pf-registry::transitive_blob_digests` walks the world FsTree to
  enumerate file blobs and the cache PageManifest to enumerate K/V page
  blobs; without this, push only mirrored the 8 top-level layer
  descriptors and `pf checkout` post-pull failed missing-blob.
- `pf-registry::sign`: cosign-shaped manifest signing. v1 ships
  `hmac-sha256` (self-signed with a default key; documented in
  `SECURITY.md` as forge-able by anyone holding the default key).
  Sigstore Fulcio (keyless) is feature-gated for v1.1.
- `pf-registry::HfRegistry`, `S3Registry`, `IpfsRegistry`: trait
  surface + URL parsing + auth-token plumbing. Live HTTP paths land in
  v1.0.1 behind their respective `*-live` feature flags.
  `pf_registry::open(image_ref, auth)` dispatches to the right adapter.
- **CLI wiring**: `crates/pf-cli/src/commands/stub.rs` (renamed
  conceptually but kept on disk for v1) now calls into `pf-registry`
  for `push`, `pull`, and `clone`. UnsupportedScheme errors map to the
  same exit-code-2 semantics as the Phase-8 stubs. Single-shot tokio
  runtime spun up per invocation.
- 8 integration tests in `crates/pf-registry/tests/registry_round_trip.rs`:
  full round-trip via FileRegistry, tampered-manifest detection,
  tampered-blob detection, two-push CoW dedup, and three "adapter
  cleanly returns UnsupportedScheme in default build" tests.
- 12 unit tests across `image_ref` and `sign`.
- 2 new CLI integ tests: `push_to_hf_exits_2_unsupported_scheme` (the
  Phase-8 stub test reworked) and `push_then_pull_via_file_registry_round_trips`
  (end-to-end CLI round-trip).

### Added — Phase 8 (CLI)

- `pf` CLI is now wired end-to-end: every subcommand from
  `agent_docs/cli-spec.md` calls into the layer crates instead of the
  Phase-0 "scaffold only" stub. Exit codes follow the spec table:
  `0` ok / `1` bad input / `2` not-yet-implemented / `3` merge conflict /
  `4` integrity failure.
- Refactored `crates/pf-cli/src/main.rs` from a single-file scaffold
  into a `commands/` module tree — one file per subcommand for
  testability.
- **Wired subcommands**:
  - `pf snapshot --agent-id <kind> --fs-root <path> [--name N] [--trace-from-jsonl PATH]`
    captures the world layer (via `pf_world::WalkFsCapture`), env (via
    `std::env::vars`), an optional JSONL trace, and stub model + cache +
    effects layers (matching the SDK's snapshot shape). Prints CID.
  - `pf fork <CID> -n <N> [--explore HINT] [--name PREFIX]` clones the
    manifest with new fingerprints and `parents = [<source>]`. CoW
    inherits all layer blobs.
  - `pf checkout <CID> --into <PATH>` calls `pf_world::restore_tree`.
  - `pf merge <FROM> --into <INTO> [--alpha 0.5] [--dare-p 0.7] [--seed N]`
    runs the Phase-6 engine with `StubSummarizer`. Exits 3 on
    `MergeOutcome::Conflicted` per the spec.
  - `pf log [--graph] [--max N]` walks `iter_manifests`, sorted newest
    first.
  - `pf diff <A> <B>` per-layer digest diff with `-`/`+` lines.
  - `pf status` shows store path, manifest count, blob bytes (+ MiB).
  - `pf gc [--retain-recent N] [--dry-run]` mark-and-sweep over
    orphaned blobs.
  - `pf verify [--deep]` re-hashes every blob via `BlobStore::get`
    (which already validates on read).
  - `pf completions <shell>` emits a `clap_complete`-generated
    script (bash / zsh / fish / powershell / elvish).
- **Stub subcommands** (Phase-9 deferred): `push`, `pull`, `clone`
  exit 2 with a clear pointer to `claude-progress.json` phase 9.
- Global flags: `--store <path>` (env `PF_STORE`, default
  `~/.processfork`), `--no-color`, `-v[vvv]`.
- 11 integration tests (`crates/pf-cli/tests/cli_smoke.rs`) using
  `assert_cmd` against the real `pf` binary, covering every wired
  subcommand + the stub exit codes + the bad-CID error path.
- `examples/02-cli-snapshot/run.sh` — runnable end-to-end demo
  exercising snapshot → status → log → snapshot → diff → checkout →
  verify → push (deferred). Exit 0 with full transcript.

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
