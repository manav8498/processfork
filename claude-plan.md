# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

**6 of 12 phases complete and tagged. 91 tests pass workspace-wide. Lints
clean. Workspace is at HEAD = `phase-5-complete`.**

| Phase | Name              | Status | Tag                | Tests |
|-------|-------------------|--------|--------------------|-------|
| 0     | bootstrap         | ✅ done | phase-0-complete  | —     |
| 1     | core_engine_rust  | ✅ done | phase-1-complete  | 16    |
| 2     | world_layer       | ✅ done | phase-2-complete  | 12    |
| 3     | effects_layer     | ✅ done | phase-3-complete  | 18    |
| 4     | cache_layer       | ✅ done | phase-4-complete  | 21    |
| 5     | model_layer       | ✅ done | phase-5-complete  | 24    |
| 6     | merge_engine      | ▶ next | —                  | —     |
| 7–12  | …                 | ⏳ pend | —                  | —     |

Headline numbers (build-host = macOS arm64, no GPU):
- Snapshot p99      : **8 ms** (budget 500 ms; ~60× headroom)
- CoW dedup growth  : **614 B** on identical-content second snapshot of 1.4 MB fixture
- 12-fork ratio     : well under 1.5× one-fork (Phase-1 + Phase-4 gates met)
- FS round-trip     : byte-identical on 32 MiB / 256-file sandbox
- Effect ledger     : tampering caught; 1000-case proptest sweep passes
- Cache layer       : byte-identical round-trip via FsBlobStore + 100-case proptest
- Model layer       : 4 diff variants round-trip; TIES + DARE primitives pass

## What's next (top of stack — Phase 6: merge engine)

Phase 6 is **three-way merge engine**. Spec lives in
`agent_docs/merge-protocol.md`.

The four primitives, one per layer:

1. **Trace** (`crates/pf-merge/src/trace.rs`): given trace blobs A, B, X,
   produce a "lessons learned" patch via the configured summarizer
   (default `claude-haiku-4-5` via `PF_SUMMARIZER` env). Phase 6 ships the
   interface (`trait Summarizer`) plus a `StubSummarizer` test impl that
   concatenates the divergence diff. Real Claude API call is gated behind
   the `live-summarizer` feature flag (no API key in build env).

2. **World** (`crates/pf-merge/src/world.rs`): three-way file diff over
   the `pf_world::FsTree` blob format. For each path in
   `union(A, B, X)` apply the table from `agent_docs/merge-protocol.md`
   §"World"; conflicts produce a `<<<<<<<`-style merge marker text blob
   per file. Output is a new `FsTree` plus a `Vec<ConflictedPath>`.

3. **Effects** (`crates/pf-merge/src/effects.rs`): never replay
   irreversible. Union of A's and B's ledgers in causal order; B's
   irreversible entries marked `replayed=false, reason="merged from
   sibling"`. Honours `--replay-effects=<class>` from the CLI.

4. **Model** (`crates/pf-merge/src/model.rs`): wrap `pf_model::ties_merge`
   and `pf_model::dare`. For LoRA we elementwise-merge the A and B
   matrices; for Full we merge each parameter; IA³ similar. If both A and
   B are non-trivial, surface a soft warning and apply task arithmetic
   with `α = 0.5`.

Plus the orchestrator:

5. **Common-ancestor discovery** (`crates/pf-merge/src/ancestor.rs`):
   walk parent chains breadth-first, find LCA. Multi-parent (octopus)
   merges error in v1.

6. **Top-level engine** (`crates/pf-merge/src/engine.rs`):
   `merge(a, b, store) -> MergeResult` runs all four layer merges and
   produces a new manifest. `MergeOutcome::Conflicted` returns enough
   info for a future `pf merge --tool` UX.

7. Integration test `tests/merge_round_trip.rs`: synthesize three small
   manifests (X, A, B) with overlapping and conflicting world-layer
   files, run merge, assert clean-merge / conflict cases match the
   table.

## Blockers

- **None for Phase 6.** Live-summarizer test is feature-gated; conflict
  resolution UX (`pf merge --tool`) is a Phase-8 CLI deliverable.

## Recently completed (this session series)

- Phase 0–3 (prior session): workspace, core engine, world, effects.
- Phase 4 (this session): paged KV-cache wire format, CachePager trait,
  SyntheticCachePager, capture/restore via BlobStore, 100-case proptest.
- Phase 5 (this session): ModelDiff variants (LoRA / IA³ / Full /
  InPlaceTtt), serialize/load via BlobStore, DARE + TIES primitives,
  64-case proptest on TIES merge.

## Files most likely to need editing in the next session

- `crates/pf-merge/Cargo.toml` — add serde_json, proptest (dev), tempfile (dev).
- `crates/pf-merge/src/lib.rs` — re-architect from Phase-0 stub to module tree.
- `crates/pf-merge/src/{trace,world,effects,model,ancestor,engine}.rs` (new).
- `crates/pf-merge/tests/merge_round_trip.rs` (new).
- `claude-progress.json` — flip phase 6 to done when gate passes.

## Operator-only deliverables (cannot run from build agent)

These remain blocked on operator action, not on code:
- `pip install processfork` end-to-end smoke (needs `PYPI_API_TOKEN`).
- `npm install @processfork/sdk` end-to-end smoke (needs `NPM_TOKEN`).
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
