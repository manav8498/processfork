# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

Phase 0 (bootstrap) is functionally complete on this machine:
- Cargo workspace with all 10 crates compiles clean (`cargo check --workspace` —
  zero warnings).
- `pf-core` has typed errors, `Digest256` (SHA-256, OCI form), `BlobStore`
  trait, and the v1 `.pfimg` manifest schema. 5/5 unit tests pass.
- `pf` CLI binary renders `--help` listing all 12 subcommands; each subcommand
  exits 2 with a "scaffold only" message until Phase 8 wires it up.
- Agent infra (CLAUDE.md, agent_docs/*, .claude/agents/*, .claude/skills/*,
  .claude/hooks/*) is being written; see "What's next" below.

## What's next (top of stack)

1. Finish writing all `agent_docs/` subsystem specs (architecture, feature-
   spec, cache-layer, model-layer, world-layer, effects-layer, merge-protocol,
   cli-spec, registry-spec, 7× integration-*, benchmarks, release-checklist).
2. Write the five sub-agent definitions in `.claude/agents/`.
3. Write the five skills in `.claude/skills/`.
4. Write the three hooks in `.claude/hooks/` and ensure they're executable.
5. Write SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md.
6. Make the first commit `feat: bootstrap workspace and agent infrastructure`.
7. Run the qa+security+perf gate (sub-agents). If PASS, tag `phase-0-complete`
   and flip phase 0 → done, phase 1 → in_progress in claude-progress.json.
8. Begin Phase 1 (`pf-core` real impl): on-disk CAS with sharded zstd-19 blobs,
   atomic snapshot orchestrator, end-to-end test that snapshots a synthetic
   four-layer fixture in <500 ms.

## Blockers

- None yet. Note assumption A-003: actual publish to PyPI/npm/crates.io/GHCR
  needs operator-supplied tokens; the v1.0 ship gate from a fresh env is
  blocked on operator action there. The workflows themselves are not blocked.

## Recently completed

- Initialized git repo (`main` branch).
- Created the §4.1 directory layout.
- Wrote workspace `Cargo.toml` (edition 2024, MSRV 1.85, shared deps).
- Wrote 10 crate stubs that compile clean (no warnings).
- Wrote `pf-core::digest::Digest256`, `pf-core::manifest::*`, `pf-core::cas::BlobStore` trait, typed errors.
- Wrote `pf` CLI scaffold with all 12 subcommands and clap completions wired.

## Files most likely to need editing in the next session

- `crates/pf-core/src/cas.rs` — implement `FsBlobStore` (sharded directory,
  zstd-19 compression, atomic write via temp+rename).
- `crates/pf-core/src/snapshot.rs` (new) — `Snapshotter` orchestrating the
  four layer captures into a single `.pfimg`.
- `examples/01-hello-fork/` (new) — first end-to-end example for the
  Phase-1 gate.
- `claude-progress.json` — flip current_phase to 1 once gate passes.

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
