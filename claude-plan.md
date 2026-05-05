# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

**4 of 12 phases complete and tagged. 46 tests pass workspace-wide. Lints
clean. Workspace is at HEAD = `phase-3-complete`.**

| Phase | Name              | Status | Tag                | Tests |
|-------|-------------------|--------|--------------------|-------|
| 0     | bootstrap         | ✅ done | phase-0-complete  | —     |
| 1     | core_engine_rust  | ✅ done | phase-1-complete  | 12+3+1|
| 2     | world_layer       | ✅ done | phase-2-complete  | 9+3   |
| 3     | effects_layer     | ✅ done | phase-3-complete  | 14+4  |
| 4     | cache_layer       | ▶ next | —                  | —     |
| 5–12  | …                 | ⏳ pend | —                  | —     |

Headline numbers (build-host = macOS arm64, no GPU):
- Snapshot p99      : **8 ms** (budget 500 ms; ~60× headroom)
- CoW dedup growth  : **614 B** on identical-content second snapshot of 1.4 MB fixture
- 12-fork ratio     : well under 1.5× one-fork (Phase-1 gate met)
- FS round-trip     : byte-identical on 32 MiB / 256-file sandbox
- Effect ledger     : tampering caught; 1000-case proptest sweep passes for all 4 invariants

## What's next (top of stack — Phase 4: cache layer)

Phase 4 is **cache layer**. The spec lives in `agent_docs/cache-layer.md` and
`.claude/skills/kvcache-format/SKILL.md`.

1. `crates/pf-cache/src/format.rs`: `PageManifest` (`paged-batchinvariant-v1`
   wire format), `Page { ix, k_digest, v_digest }`, `LogicalSeq { id, page_ixs,
   fill_in_last_page }`.
2. `crates/pf-cache/src/serialize.rs`: portable `serialize_pages(pages: &[Page])`
   / `deserialize_pages(manifest: &PageManifest, blobs)` round-trip — uses the
   `BlobStore` trait, no GPU. Exercises CAS dedup across forks naturally.
3. `crates/pf-cache/src/vllm_adapter.rs` (gated by feature `vllm-adapter` so it
   doesn't pull vLLM into non-Python builds): pause→read-pages→hash→stream→
   resume FFI shim. Real wiring deferred to Phase 10's vLLM integration; for
   Phase 4 ship the **interface** (`trait CachePager`) plus a synthetic
   in-memory implementation that lets `tests/` exercise the round-trip.
4. `crates/pf-cache/src/sglang_adapter.rs`: same shape as vLLM, `RadixAttention`
   prefix-sharing preserved through `LogicalSeq.page_ixs`.
5. Integration test `tests/cache_round_trip.rs`: 100 random page sets ×
   serialize → deserialize → assert byte-identical (proxy for the bit-exact
   replay test which needs a CUDA host gated by `$PF_HAS_GPU=1`).
6. Write the GPU-gated `tests/cache_bit_exact_vllm.rs` skeleton that
   `eprintln!`-skips when `PF_HAS_GPU != 1` so operators on a CUDA host
   can run it without code changes.

## Blockers

- **None for Phase 4.** Real bit-exact replay against vLLM/SGLang requires a
  CUDA host; that test is structured to skip cleanly off-GPU and run cleanly
  on-GPU when the operator drops it onto a Hopper-class box.

## Recently completed (this session)

- Phase 0: workspace skeleton (10 crates), all 13 agent_docs, 5 sub-agents,
  5 skills, 3 hooks, project meta files, CI workflow.
- Phase 1: real `FsBlobStore` (sharded zstd-19 CAS, atomic write,
  corruption-detecting), `PfStore`, `Snapshotter`, synthetic fixtures,
  `examples/01-hello-fork`. **8 ms snapshot, 614 B dedup growth.**
- Phase 2: portable `WalkFsCapture` (rayon-parallel, deterministic,
  ignore-list, APFS-clone fast-path opt-in), atomic `restore_tree`,
  `EnvCapture` (with regex scrub), `ProcsCapture` (CRIU stub on Linux,
  `Unsupported` placeholder on macOS).
- Phase 3: append-only `Ledger` with HMAC-chained entries, `SessionSecret`
  with redacted Debug, `ToolProxy` interceptor, `ReplayPolicy` with three
  presets, **1000-case proptest fuzzer for all four ACRFence invariants**.

## Files most likely to need editing in the next session

- `crates/pf-cache/Cargo.toml` — add `serde_json`, `bytes`, `proptest` (dev),
  `tempfile` (dev). The `vllm-adapter` feature gate keeps vLLM out of the
  default build matrix.
- `crates/pf-cache/src/lib.rs` — re-architect from Phase-0 stub to module tree.
- `crates/pf-cache/src/format.rs`, `src/serialize.rs`, `src/synthetic.rs`,
  `src/{vllm,sglang}_adapter.rs` (new).
- `crates/pf-cache/tests/cache_round_trip.rs`, `tests/cache_bit_exact_vllm.rs`
  (new; gated).
- `claude-progress.json` — flip phase 4 to done when gate passes.

## Operator-only deliverables (cannot run from build agent)

These remain blocked on operator action, not on code:
- `pip install processfork` end-to-end smoke (needs `PYPI_API_TOKEN`).
- `npm install @processfork/sdk` end-to-end smoke (needs `NPM_TOKEN`).
- `cargo install processfork` from crates.io (needs `CARGO_REGISTRY_TOKEN`).
- 60-second asciinema demo recording (script lives under `demo/` once it's
  written in Phase 12; recording is operator-produced).
- Real-hardware bit-exact replay test (needs CUDA host + Llama-3-8B served by
  vLLM ≥0.10 in deterministic mode; gated behind `$PF_HAS_GPU=1`).

## Context-window discipline reminders

- 60 % → write a one-paragraph progress note here.
- 70 % → commit WIP behind a feature flag if needed; consider compact.
- 85 % → finish the current logical unit; stop adding new work; leave clean
  state files for the next session.
