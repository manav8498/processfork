# ProcessFork

> **`fork()` for AI agents.** Snapshot a live agent — model fast-weights,
> KV-cache, sandbox filesystem, tool-effect ledger, and reasoning trace —
> into one content-addressed image. Branch it. Merge it. Push it.
> Replay it.

ProcessFork is to AI agents what `git` is to source code. An "agent"
today is a smear of state across at least five processes — the model
server, the KV cache, the sandbox FS, the open browser, and a pile of
side-effects in external systems. ProcessFork makes an agent a
**first-class object** you can hold in your hand: snapshot it, fork it,
merge it, push it to a registry, clone it on another machine.

## What that buys you

| capability                              | enabled by                          |
|-----------------------------------------|-------------------------------------|
| Try N alternative approaches in parallel| `pf fork <CID> -n N`                |
| Restart from a known-good state         | `pf checkout <CID>`                 |
| Time-travel debug a stuck agent         | `pf log` + `pf checkout`            |
| Hand a live session to a teammate       | `pf push hf://you/session-2026-…`   |
| Atomically merge two divergent threads  | `pf merge B --into A`               |
| Idempotent rollouts (RL fabric)         | content-addressed CoW across forks  |

## The four layers

| Layer       | What it captures                                         |
|-------------|----------------------------------------------------------|
| **Model**   | LoRA / IA³ / full-finetune weight diffs, In-Place TTT    |
| **Cache**   | Paged KV-cache, content-addressed, copy-on-write         |
| **World**   | FS (overlayfs / APFS clones), env, in-flight subprocs    |
| **Effects** | Append-only ledger of irreversible tool calls            |

Plus the reasoning **trace** for typed, effect-aware three-way merge.

See [Architecture overview](./architecture.md) for the full design.

## Status

v1.0 ships:

- The full four-layer atomic-snapshot model.
- The 12-subcommand `pf` CLI.
- Python and TypeScript SDKs (Rust core via pyo3 / napi-rs).
- Five registry adapters (file, HF Hub, S3, IPFS, OCI) — file
  end-to-end, others scaffolded for v1.0.1.
- Seven first-party integration adapters (Claude Code, LangGraph,
  OpenInterpreter, vLLM, SGLang, AutoGen, CrewAI) — three end-to-end,
  four scaffolded.
- 200+ tests across Rust + Python + TypeScript surfaces.

See [v1.0 release checklist](./release-checklist.md) for the full
ship-gate matrix and what's deferred to v1.0.1 / v1.1.
