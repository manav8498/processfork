# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
