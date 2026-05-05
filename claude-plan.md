# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

**8 of 12 phases complete and tagged. 132 tests pass (122 Rust + 5 Python +
5 TypeScript). Lints clean. Workspace is at HEAD = `phase-7-complete`.**

| Phase | Name              | Status | Tag                | Tests   |
|-------|-------------------|--------|--------------------|---------|
| 0     | bootstrap         | ✅ done | phase-0-complete  | —       |
| 1     | core_engine_rust  | ✅ done | phase-1-complete  | 16      |
| 2     | world_layer       | ✅ done | phase-2-complete  | 12      |
| 3     | effects_layer     | ✅ done | phase-3-complete  | 18      |
| 4     | cache_layer       | ✅ done | phase-4-complete  | 21      |
| 5     | model_layer       | ✅ done | phase-5-complete  | 24      |
| 6     | merge_engine      | ✅ done | phase-6-complete  | 31      |
| 7     | sdks              | ✅ done | phase-7-complete  | 5+5     |
| 8     | cli               | ▶ next | —                  | —       |
| 9–12  | …                 | ⏳ pend | —                  | —       |

Phase 7 deliverables (this session):
- **Python SDK**: built via maturin into a real wheel, installed in a
  fresh uv venv (Py 3.12), 5/5 smoke tests pass.
- **TypeScript SDK**: built via napi-rs into `processfork.darwin-arm64.node`,
  5/5 `node --test` smoke tests pass.

## What's next (top of stack — Phase 8: CLI)

Phase 8 is the **`pf` CLI** — wire all 12 subcommands from
`agent_docs/cli-spec.md` to the layer crates.

Current state: `crates/pf-cli/src/main.rs` already has clap derive
parsing for all 12 subcommands and renders `--help` correctly; each
subcommand currently exits 2 with a "scaffold only" message.

What needs wiring per `agent_docs/cli-spec.md`:

1. `pf snapshot <agent-id>` — for v1 we ship the simple flow used by
   the Python/TS SDKs: walk a `--fs-root`, capture env, attach an
   optional `--trace-from-jsonl` file. (Real adapter integration —
   "snapshot the running Claude Code session" — is Phase 10.)
2. `pf fork <CID> -n <count>` — copy the manifest with new fingerprints;
   for v1 the spawn-N-live-branches surface is via the SDK adapters.
3. `pf checkout <CID> --into <PATH>` — call `pf_world::restore_tree`.
4. `pf merge <FROM> --into <INTO>` — call `pf_merge::merge` with
   `StubSummarizer`.
5. `pf push <CID> <TARGET>` — Phase-9 work; for Phase 8 wire to a
   `Result::Err(Unimplemented)` with a clear message and exit code 2.
6. `pf pull <SOURCE>` — same.
7. `pf clone <SOURCE>` — same.
8. `pf log [--graph] [--max N]` — walk `store.iter_manifests()`.
9. `pf diff <A> <B>` — load both manifests and pretty-print the
   per-layer digests + a one-line summary per layer.
10. `pf status` — store size, manifest count, default location.
11. `pf gc [--retain-recent N] [--dry-run]` — basic mark-and-sweep:
    walk manifests, compute reachable set, delete orphan blobs.
12. `pf verify [--deep]` — re-hash every blob; fail on mismatch.

Plus shell completions via `clap_complete`: `pf completions bash|zsh|fish`.

Integration test: `crates/pf-cli/tests/cli_smoke.rs` invokes the binary
via `assert_cmd` (or `std::process::Command`) for the major subcommands.
End-to-end example: `examples/02-cli-snapshot/` showing
snapshot → checkout via the CLI.

## Blockers

- **None for Phase 8.** Push/pull are deferred to Phase 9 (registry).

## Recently completed (this session)

- Phase 6 (merge engine): six modules — ancestor + trace + world +
  effects + model + engine. 28 unit + 3 integration tests.
- Aligned the Phase-1 fixtures with Phase-2/3/5 wire formats so the
  synthetic fork-pair flows through the engine.
- Phase 7 (Python + TypeScript SDKs): both built end-to-end against
  real maturin/napi binaries; 10 smoke tests pass.

## Files most likely to need editing in the next session

- `crates/pf-cli/src/main.rs` — flesh out subcommand handlers.
- `crates/pf-cli/src/commands/{snapshot,checkout,merge,log,diff,status,gc,verify}.rs`
  (new) — one file per command for testability.
- `crates/pf-cli/tests/cli_smoke.rs` (new).
- `examples/02-cli-snapshot/` (new).
- `claude-progress.json` — flip phase 8 to done when gate passes.

## Operator-only deliverables (cannot run from build agent)

These remain blocked on operator action, not on code:
- `pip install processfork` end-to-end smoke from PyPI (needs
  `PYPI_API_TOKEN`). The wheel itself builds and installs locally.
- `npm install @processfork/sdk` end-to-end smoke from npm (needs
  `NPM_TOKEN`). The .node binary builds and runs locally.
- `cargo install processfork` from crates.io (needs `CARGO_REGISTRY_TOKEN`).
- 60-second asciinema demo recording (script lives under `demo/` once it's
  written in Phase 12; recording is operator-produced).
- Real-hardware bit-exact replay test (needs CUDA host + Llama-3-8B served by
  vLLM ≥0.10 in deterministic mode; gated behind `$PF_HAS_GPU=1`).
- mergekit-equivalence test (needs Llama-3-8B base weights + Python
  `mergekit` install; gated behind `$PF_HAS_GPU=1`).
- Live summarizer call for trace-merge (needs Anthropic API key; gated
  behind the `live-summarizer` feature flag).

## Context-window discipline reminders

- 60 % → write a one-paragraph progress note here.
- 70 % → commit WIP behind a feature flag if needed; consider compact.
- 85 % → finish the current logical unit; stop adding new work; leave clean
  state files for the next session.
