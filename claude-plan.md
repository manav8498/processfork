# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

Phases 0 (bootstrap) and 1 (core engine, Rust) are complete and tagged.
Phase 2 (world layer) is starting.

- Workspace is 10 crates + 1 example, clean fmt/clippy/test on macOS arm64.
- `pf-core` is the load-bearing crate: `Digest256`, `Manifest` v1,
  `FsBlobStore` (sharded zstd-19 CAS, on-read re-hash), `MemBlobStore`,
  `PfStore` (CAS + manifest catalog), `Snapshotter` (atomic 4-layer with
  `thread::scope`), `fixture::*` synthetic captures.
- 16 tests pass (12 unit + 3 integration + 1 doctest).
- `examples/01-hello-fork` runs end-to-end: snapshot in **8 ms**, 614 B
  growth on identical-content second snapshot (CoW dedup proven).

## What's next (top of stack)

1. **Phase 2 — world layer.** In `crates/pf-world/`:
   - `Snapshot` trait with concrete impls per filesystem backend.
   - `OverlayfsCapture` (Linux, gated by `cfg(target_os = "linux")`).
   - `ApfsCloneCapture` (macOS, via `nix::sys::clonefile` or shelling out
     to `cp -c`).
   - `WalkFsCapture` portable fallback (rayon-parallel hashing, mtime+size
     pre-filter).
   - `EnvCapture` (`std::env::vars()` + cwd, with `--scrub-env` regex).
   - `ProcsCapture` — Linux: shell out to `criu dump`; macOS: write the
     `unsupported_on: darwin` placeholder per agent_docs/world-layer.md.
   - `BrowserCapture` (CDP) — stub for v1; real impl behind a feature flag.
2. Replace the synthetic `FixtureWorldCapture` with a real `WalkFsCapture`
   over a temp directory in the world-layer integration test.
3. Wire the new `WorldCapture` into `Snapshotter` via the existing
   `LayerCapture` trait.
4. Examples/02 (12-way parallel) starts after Phase 6 (merge engine) lands;
   examples/03+ require GPU; document those gates clearly in their READMEs.

## Blockers

- None. Phase 2 has no external dependencies; CRIU subprocess capture is
  Linux-only and will be `#[cfg(target_os = "linux")]`-gated, with the macOS
  build host emitting the documented "unsupported_on: darwin" placeholder.

## Recently completed

- Phase 0: workspace skeleton, agent infrastructure, first commit + tag.
- Phase 1: real `FsBlobStore` (sharded, zstd-19, atomic, corruption-detecting),
  `PfStore`, `Snapshotter`, `fixture::*`, integration tests, hello-fork
  example. Snapshot p99 = 8 ms vs 500 ms budget.

## Files most likely to need editing in the next session

- `crates/pf-world/src/lib.rs` — re-architect from Phase-0 stub to module tree.
- `crates/pf-world/src/walk.rs` (new) — portable rayon-parallel FS walker.
- `crates/pf-world/src/apfs.rs` (new, macOS-only) — `clonefile(2)` capture.
- `crates/pf-world/src/overlayfs.rs` (new, linux-only) — overlayfs capture.
- `crates/pf-world/src/env.rs` (new) — env-var capture with `--scrub-env`.
- `crates/pf-world/src/procs.rs` (new) — CRIU dump on Linux, stub on macOS.
- `crates/pf-world/tests/` — round-trip tests for each backend.
- `claude-progress.json` — flip phase 2 to done when gate passes.

## Context-window discipline reminders

- 60% → write a one-paragraph progress note here.
- 70% → commit WIP behind a feature flag if needed; consider compact.
- 85% → finish the current logical unit; stop adding new work; leave clean
  state files for the next session.

## Operator-only deliverables (cannot run from build agent)

- `pip install processfork` end-to-end smoke (needs `PYPI_API_TOKEN`).
- `npm install @processfork/sdk` end-to-end smoke (needs `NPM_TOKEN`).
- `cargo install processfork` from crates.io (needs `CARGO_REGISTRY_TOKEN`).
- 60-second asciinema demo recording (script under `demo/`, recording is
  operator-produced; harness cannot record its own terminal).
- Real-hardware bit-exact replay test (needs CUDA host + Llama-3-8B served by
  vLLM ≥0.10 in deterministic mode; gated behind `$PF_HAS_GPU=1`).
