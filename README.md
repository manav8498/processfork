# ProcessFork

> **`fork()` for AI agents.** Snapshot a live agent — model fast-weights,
> KV-cache, sandbox filesystem, tool-effect ledger, and reasoning trace —
> into one content-addressed image. Branch it. Merge it. Push it. Replay it.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Status: pre-alpha](https://img.shields.io/badge/status-pre--alpha-red.svg)](#status)

> ⚠️ **Status: pre-alpha (v0.1.0-dev).** This repository is the initial
> bootstrap of the ProcessFork project. The four-layer snapshot model and
> CLI surface are scaffolded; many subsystems are not yet implemented. See
> [`claude-progress.json`](claude-progress.json) for the live build state and
> [`claude-plan.md`](claude-plan.md) for what's next. The targeted v1.0 spec
> lives in [`agent_docs/feature-spec.md`](agent_docs/feature-spec.md).

## What it is

ProcessFork is to AI agents what `git` is to source code. An "agent" today is
a smear of state across at least five processes (the model server, the KV
cache, the sandbox FS, the open browser, and a pile of side-effects in
external systems). ProcessFork makes an agent a **first-class object** you can
hold in your hand: snapshot it, fork it, merge it, push it to a registry,
clone it on another machine.

## The four layers

| Layer       | What it captures                                         |
|-------------|----------------------------------------------------------|
| **Model**   | LoRA / IA³ / full-finetune weight diffs, In-Place TTT    |
| **Cache**   | Paged KV-cache, content-addressed, copy-on-write         |
| **World**   | FS (overlayfs / APFS clones), env, in-flight subprocs    |
| **Effects** | Append-only ledger of irreversible tool calls            |

Plus the reasoning **trace** for typed, effect-aware three-way merge.

## Install (planned, not yet published)

```bash
cargo install processfork          # Rust CLI
pip   install processfork          # Python SDK
npm   install @processfork/sdk     # TypeScript SDK
```

## Hello, fork (planned API)

```bash
pf snapshot my-agent                       # → bafy...abc
pf fork bafy...abc -n 12 --explore "fix"   # 12 divergent live branches
pf merge winner-3 -> main                  # typed three-way merge
pf push hf://user/refactor-2026-05-05      # ship to Hugging Face Hub
```

## Repository layout

```
crates/      Rust workspace (pf-core, pf-cache, pf-world, …, pf-cli, pf-py, pf-ts)
adapters/    Per-framework integrations (Claude Code, LangGraph, vLLM, …)
benchmarks/  PFBench (SWE-Bench / GAIA) and microbenchmarks
docs/        mdBook source
examples/    Eight self-contained runnable examples
landing/     GitHub Pages site
demo/        60-second viral demo recording scripts
agent_docs/  Subsystem specifications (loaded on demand by the build agent)
.claude/     Sub-agents, skills, hooks for the build agent
```

## Building from source

```bash
cargo check --workspace
cargo test  --workspace
```

## License

MIT. See [LICENSE](LICENSE).
