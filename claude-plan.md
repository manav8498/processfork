# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

**10 of 12 phases complete and tagged. 164 tests pass (154 Rust + 5 Python +
5 TypeScript). Lints clean. Workspace is at HEAD = `phase-9-complete`.**

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
| 8     | cli               | ✅ done | phase-8-complete  | 12      |
| 9     | registry          | ✅ done | phase-9-complete  | 20      |
| 10    | integrations      | ▶ next | —                  | —       |
| 11–12 | …                 | ⏳ pend | —                  | —       |

End-to-end registry round-trip works on the build host:
- `pf snapshot` in store A → `pf push file://...` to a registry dir →
  `pf pull file://...` into store B → CID is identical.
- `pf clone file://... --into PATH` does pull + restore in one step.
- Tampered manifests / blobs caught on pull (signature + re-hash).

## What's next (top of stack — Phase 10: integrations)

Phase 10 is the **seven first-party integration adapters** per
`agent_docs/feature-spec.md` M5. Each adapter wraps an existing
agent-runtime so it can snapshot / fork / merge through ProcessFork.

The full v1 list:
1. Claude Code wrapper (`pf wrap claude` slash-commands).
2. LangGraph checkpointer.
3. OpenInterpreter wrapper.
4. vLLM native server plugin (paged-KV cache via Phase-4 format).
5. SGLang native server plugin (RadixAttention via Phase-4 format).
6. AutoGen runtime adapter.
7. CrewAI memory adapter.

For one session, realistic scope:
- Lay down the seven crate skeletons under `adapters/`.
- Ship 2–3 adapters end-to-end; scaffold the rest with API surface +
  README + GPU-/network-gated test placeholders.
- Each adapter's spec lives in `agent_docs/integration-<name>.md`.

**Recommended order**:
1. **Claude Code** — pure-Python wrapper around the SDK; no model
   server needed; testable on the build host.
2. **LangGraph** — Python adapter implementing the
   `langgraph.checkpoint.BaseCheckpointSaver` interface; heavy dep but
   testable with a tiny synthetic graph.
3. **OpenInterpreter** — pure-Python; should follow Claude Code's
   pattern.
4. **vLLM**, **SGLang**, **AutoGen**, **CrewAI** — need real model
   servers / multi-agent runtimes; ship API + integration test
   skeletons gated by `$PF_HAS_GPU=1` (vLLM/SGLang) or just deps
   present (AutoGen/CrewAI).

## Blockers

- **None for the recommended scope** above. Real Llama-3-8B integration
  for vLLM/SGLang requires a CUDA host and is gated by `$PF_HAS_GPU=1`.

## Recently completed (this session)

- Phase 8 (CLI): refactored main.rs into commands/ tree; wired 10
  subcommands; stubbed 3 to Phase 9; 11 assert_cmd integ tests;
  examples/02-cli-snapshot/run.sh.
- Phase 9 (registry): ImageRef parser for 5 schemes; Registry trait;
  FileRegistry full impl with sign+verify; HF/S3/IPFS scaffolded;
  transitive blob walker; CLI push/pull/clone wired end-to-end;
  20 tests in pf-registry.

## Files most likely to need editing in the next session

- `adapters/pf-claude-code/` (new) — pure-Python wrapper around the
  Python SDK + a hook script for Claude Code's PreToolUse / PostToolUse.
- `adapters/pf-langgraph/` (new) — Python pkg.
- `adapters/pf-openinterpreter/` (new).
- `adapters/pf-{vllm,sglang,autogen,crewai}/` — API surface + README.
- `examples/03-claude-code-fork/`, `examples/04-langgraph-checkpoint/`,
  etc. (one per shipped adapter).
- `claude-progress.json` — flip phase 10 to done when gate passes.

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
- Live HF Hub push/pull (needs `HF_TOKEN`; gated `--features hf-live`).
- Live S3 push/pull (needs AWS creds; gated `--features s3-live`).
- Live IPFS push/pull (needs local IPFS daemon; gated
  `--features ipfs-live`).

## Context-window discipline reminders

- 60 % → write a one-paragraph progress note here.
- 70 % → commit WIP behind a feature flag if needed; consider compact.
- 85 % → finish the current logical unit; stop adding new work; leave clean
  state files for the next session.
