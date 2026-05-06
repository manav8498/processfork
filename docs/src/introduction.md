# ProcessFork

> **`git` for AI agents.** Snapshot, fork, and merge live LLM sessions in **8 ms**.

You're 4 hours into a refactor with Claude Code. The agent has read 200 files, run 47 tests, opened a database, started a dev server. Then it suggests a destructive change.

**Today**: lose everything, undo by hand, or restart.
**With ProcessFork**: `pf snapshot` → 8 ms → safe. Try 12 alternatives in parallel, merge the winner back, ship the whole session to a teammate.

```
                                 ┌─→ attempt #1   (broke build)
                                 ├─→ attempt #2   (looks promising)
   4-hour                        ├─→ attempt #3   ←── winner
   Claude session   ─snapshot─→  ├─→ attempt #4
                                 ├─→ attempt #5   (also broke)
                                 ├─→ ...
                                 └─→ attempt #12
                                       │
   continue from ←───── merge ─────────┘
   the winner
```

## Highlights

- ⚡ **8 ms snapshots** (synthetic fixture); **42 ms p50** on real GPU host (Modal A10G). Full agent state — model + KV-cache + files + tools + reasoning — into one content-addressed `.pfimg`.
- 🎯 **Bit-exact verified.** 38 619 KV pages snapshotted from a live vLLM-served TinyLlama-1.1B, restored byte-identical on a clean machine, regenerated text matched. See [`benchmarks/gpu-validation/`](https://github.com/manav8498/processfork/tree/main/benchmarks/gpu-validation).
- 🌳 **Real fork & merge.** 12 parallel attempts share storage automatically (CoW). Merge the winner with a real 3-way diff (files, tools, trace) — git-style `<<<<<<<` markers and all.
- 🔒 **Won't double-send your email.** HMAC-chained tool-call ledger; restored agents see prior side-effects as facts, not as actions to re-issue. (ACRFence-resistant.)
- 🤝 **Drop-in for** Claude Code, LangGraph, OpenInterpreter, vLLM, SGLang, AutoGen, CrewAI.
- 📦 **Single binary**, MIT, Rust core, Python + TypeScript SDKs. **200+ tests.**

## When you'd reach for it

| Situation                                       | Command                                  |
|-------------------------------------------------|------------------------------------------|
| Agent about to do something destructive         | `pf snapshot pre-rm-rf`                  |
| Stuck — want to try 12 approaches in parallel   | `pf fork -n 12 --explore "fix bug"`      |
| Hand off a complex session to a teammate        | `pf push hf://you/session-name`          |
| Time-travel debug ("when did it go wrong?")     | `pf log` then `pf checkout <CID>`        |
| RL rollout fabric (agent training)              | snapshot, fan out, score, merge          |

## How it works

ProcessFork captures the **five things** that together make up a live agent — atomically — into one content-addressed file:

| Layer       | What it captures                                                |
|-------------|-----------------------------------------------------------------|
| **Model**   | LoRA / IA³ / full-finetune weight diffs, in-place TTT updates   |
| **Cache**   | Paged KV-cache, content-addressed per page (CoW across forks)   |
| **World**   | Filesystem, env, in-flight subprocesses, browser DOM            |
| **Effects** | Append-only ledger of irreversible tool calls (HMAC-chained)    |
| **Trace**   | Chat + tool-call message log                                    |

Identical content shares storage automatically — twelve parallel forks use about 1.5× the space of one, not 12×. The merge engine handles each layer with the right algorithm: git-style 3-way diff for files, TIES + DARE for model weights, an HMAC chain that defends against semantic-rollback attacks (ACRFence), and an LLM-summarized "what branch B learned" patch injected into branch A's reasoning trace without re-prefilling the cache.

See [Architecture overview](./architecture.md) for the full design.

## Status

**v1.0** ships:

- ✅ Atomic four-layer snapshot model
- ✅ The 12-subcommand `pf` CLI
- ✅ Python (pyo3) and TypeScript (napi-rs) SDKs
- ✅ Five registry adapters (file end-to-end; HF / S3 / IPFS / OCI scaffolded for v1.0.1)
- ✅ Seven first-party integration adapters (Claude Code / LangGraph / OpenInterpreter / AutoGen / CrewAI today; vLLM / SGLang scaffolded for v1.0.1)
- ✅ 200+ tests across Rust + Python + TypeScript

Ready to try it? Start with [Install](./install.md) → [Your first fork](./first-fork.md).
