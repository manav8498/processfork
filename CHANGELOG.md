# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
