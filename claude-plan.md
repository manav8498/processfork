# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

**5 of 12 phases complete and tagged. 67 tests pass workspace-wide. Lints
clean. Workspace is at HEAD = `phase-4-complete`.**

| Phase | Name              | Status | Tag                | Tests |
|-------|-------------------|--------|--------------------|-------|
| 0     | bootstrap         | ✅ done | phase-0-complete  | —     |
| 1     | core_engine_rust  | ✅ done | phase-1-complete  | 16    |
| 2     | world_layer       | ✅ done | phase-2-complete  | 12    |
| 3     | effects_layer     | ✅ done | phase-3-complete  | 18    |
| 4     | cache_layer       | ✅ done | phase-4-complete  | 21    |
| 5     | model_layer       | ▶ next | —                  | —     |
| 6–12  | …                 | ⏳ pend | —                  | —     |

Headline numbers (build-host = macOS arm64, no GPU):
- Snapshot p99      : **8 ms** (budget 500 ms; ~60× headroom)
- CoW dedup growth  : **614 B** on identical-content second snapshot of 1.4 MB fixture
- 12-fork ratio     : well under 1.5× one-fork (Phase-1 + Phase-4 gates met)
- FS round-trip     : byte-identical on 32 MiB / 256-file sandbox
- Effect ledger     : tampering caught; 1000-case proptest sweep passes
- Cache layer       : byte-identical round-trip via FsBlobStore + 100-case proptest

## What's next (top of stack — Phase 5: model layer)

Phase 5 is **model layer**. Spec lives in `agent_docs/model-layer.md`.

1. `crates/pf-model/src/diff.rs`: `ModelDiff` enum + per-variant types:
   - `Lora { adapters: Vec<LoRAAdapter> }` where `LoRAAdapter { layer_id,
     matrix, rank, in_dim, out_dim, a: Vec<f32>, b: Vec<f32> }`.
   - `IA3 { scaling: BTreeMap<LayerId, Vec<f32>> }`.
   - `Full { params: BTreeMap<ParamName, Vec<f32>> }`.
   - `InPlaceTtt { steps: Vec<TttStep> }`.
2. `crates/pf-model/src/serialize.rs`: round-trip every variant through
   `BlobStore`. Dedup-friendly: each parameter tensor hashed independently
   so two LoRA adapters that share a weight matrix share storage.
3. `crates/pf-model/src/merge.rs`: TIES + DARE task arithmetic.
   - `dare(delta, p)`: zero out a fraction `p` of magnitudes, rescale
     survivors by `1/(1-p)`. Default p=0.7 per `agent_docs/architecture.md`.
   - `ties_merge(deltas, alpha)`: trim, sign-elect (majority vote),
     disjoint-merge survivors. Default α=0.5.
   - Tested against synthetic deterministic inputs; the mergekit-equivalence
     test (against an external `mergekit` reference) is gated by
     `$PF_HAS_GPU=1` because it needs Llama-3-8B base weights.
4. Integration test `tests/model_round_trip.rs`: serialize→deserialize→
   apply→assert byte-identical post-apply state for each ModelDiff variant.
5. Optional bench: TIES+DARE wall-clock on 8×8 toy matrices (Criterion).

Trade-off note: real safetensors integration is a heavy dep (`safetensors`
crate, ~50 kloc transitively). For Phase 5 we ship the algebra + a
JSON-typed wire format; safetensors interop lands in Phase 10's vLLM
adapter where it's actually needed.

## Blockers

- **None for Phase 5.** Mergekit-equivalence test is GPU-gated; the algebra
  unit tests run on the build host.

## Recently completed (this session series)

- Phase 0–3 (prior session): workspace, core engine, world, effects.
- Phase 4 **(this session)**: paged KV-cache wire format, engine-agnostic
  CachePager trait, SyntheticCachePager (in-memory, deterministic),
  serialize/deserialize via BlobStore, capture/restore high-level helpers,
  100-case proptest round-trip, GPU-gated bit-exact-vLLM test skeleton.

## Files most likely to need editing in the next session

- `crates/pf-model/Cargo.toml` — add serde_json, proptest (dev), tempfile (dev).
- `crates/pf-model/src/lib.rs` — re-architect from Phase-0 stub to module tree.
- `crates/pf-model/src/{diff,serialize,merge}.rs` (new).
- `crates/pf-model/tests/model_round_trip.rs` (new).
- `claude-progress.json` — flip phase 5 to done when gate passes.

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

## Context-window discipline reminders

- 60 % → write a one-paragraph progress note here.
- 70 % → commit WIP behind a feature flag if needed; consider compact.
- 85 % → finish the current logical unit; stop adding new work; leave clean
  state files for the next session.
