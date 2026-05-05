# ProcessFork

**Snapshot, branch, and merge live AI agent state — like `git`, for agents.**

You're four hours into a complex refactor with Claude Code. The agent has read 200 files, run 47 tests, opened a database, started a dev server. Then it suggests a sweeping change that breaks everything.

Today, you have two options: undo by hand, or start over.

ProcessFork gives you a third:

```bash
$ pf snapshot
sha256:1c2497b0…   ← 8 ms. Your entire agent state, captured.
```

That snapshot is now a real object you can fork, merge, push to a registry, clone on a different machine. The agent's memory (KV-cache), its sandbox files, its tool-call history, its model weights — all captured atomically into one content-addressed file.

It's `git`. For AI agents.

## What you can do with it

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

| Situation                                       | What you do                              |
|-------------------------------------------------|------------------------------------------|
| Agent is about to do something destructive      | `pf snapshot pre-rm-rf` first            |
| You're stuck and want to try 12 approaches      | `pf fork -n 12 --explore "fix the bug"`  |
| Hand off a complex session to a teammate        | `pf push hf://you/session-name`          |
| Time-travel debug ("when did it go wrong?")     | `pf log` then `pf checkout <CID>`        |
| RL rollout fabric (agent training)              | Same primitive: snapshot, fan out, score |

## How it pulls it off

An "agent" at runtime is the simultaneous, mutating state of five things at once. ProcessFork captures all five **atomically**:

| Layer       | What it captures                                                |
|-------------|-----------------------------------------------------------------|
| **Model**   | LoRA / IA³ / full-finetune weight diffs, in-place TTT updates   |
| **Cache**   | Paged KV-cache, content-addressed per page (CoW across forks)   |
| **World**   | Filesystem, env vars, in-flight subprocesses, browser DOM       |
| **Effects** | Append-only ledger of every irreversible tool call              |
| **Trace**   | Chat + tool-call message log                                    |

Identical content shares storage automatically — twelve parallel forks use about 1.5× the space of one, not 12×.

The merge engine handles each layer with the right algorithm: git-style 3-way diff for files, TIES + DARE for model weights, an HMAC-chained ledger that **never re-sends your email** when you restore (defends against ACRFence-style semantic-rollback attacks), and an LLM-summarized "what branch B learned" patch that gets injected into branch A's reasoning trace without re-prefilling the cache.

See [Architecture overview](./architecture.md) for the full design.

## Status

**v1.0** ships today:

- ✅ Atomic four-layer snapshot model
- ✅ The 12-subcommand `pf` CLI
- ✅ Python (pyo3) and TypeScript (napi-rs) SDKs
- ✅ Five registry adapters (file end-to-end; HF / S3 / IPFS / OCI scaffolded for v1.0.1)
- ✅ Seven first-party integration adapters (Claude Code / LangGraph / OpenInterpreter end-to-end today; vLLM / SGLang / AutoGen / CrewAI scaffolded)
- ✅ 200+ tests across Rust + Python + TypeScript

The bit-exact replay against a real Llama-3-8B vLLM server (the kickoff frame's "snapshot a 380K-token agent in 87 ms") is the v1.0.1 deliverable — wire format and adapter API ship today, the live FFI lands next.

See the [v1.0 release checklist](./release-checklist.md) for the full ship-gate matrix and what's deferred.

Ready to try it? Start with [Install](./install.md) → [Your first fork](./first-fork.md).
