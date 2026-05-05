<h1 align="center">ProcessFork</h1>
<p align="center"><b><code>git</code> for AI agents.</b> Snapshot, fork, and merge live LLM sessions in <b>8&nbsp;ms</b>.</p>

<p align="center">
  <img src=".github/hero.svg" alt="snapshot a 4-hour Claude Code session in 8 ms, fork into 12 attempts, merge the winner back, push to a registry" width="100%">
</p>

<p align="center">
  <a href="https://github.com/manav8498/processfork/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT"></a>
  <a href="https://github.com/manav8498/processfork/actions"><img src="https://img.shields.io/github/actions/workflow/status/manav8498/processfork/ci.yml?branch=main&label=CI" alt="CI"></a>
  <a href="#status"><img src="https://img.shields.io/badge/tests-200%20passing-brightgreen" alt="200 tests"></a>
  <a href="#status"><img src="https://img.shields.io/badge/snapshot-8%20ms-brightgreen" alt="8 ms snapshot"></a>
  <a href="#install"><img src="https://img.shields.io/badge/Rust%20%2B%20Python%20%2B%20TypeScript-✓-orange" alt="Rust + Py + TS"></a>
</p>

---

## Why

You're 4 hours into a refactor with Claude Code. The agent has read 200 files, run 47 tests, opened a database, started a dev server. Then it suggests a destructive change.

**Today**: lose everything, undo by hand, or restart.
**With ProcessFork**: `pf snapshot` → 8 ms → safe. Try 12 alternatives in parallel, merge the winner back, ship the whole session to a teammate.

It's `git` — snapshot, branch, merge, push, clone — but for live AI agent state.

## Highlights

- ⚡ **8 ms snapshots.** Full agent state — model + KV-cache + files + tools + reasoning — into one content-addressed `.pfimg`.
- 🌳 **Real fork & merge.** 12 parallel attempts share storage automatically (CoW). Merge the winner with a real 3-way diff (files, tools, trace) — git-style `<<<<<<<` markers and all.
- 🔒 **Won't double-send your email.** HMAC-chained tool-call ledger; restored agents see prior side-effects as facts, not as actions to re-issue. (ACRFence-resistant.)
- 🤝 **Drop-in for** Claude Code, LangGraph, OpenInterpreter, vLLM, SGLang, AutoGen, CrewAI.
- 📦 **Single binary**, MIT, Rust core, Python + TypeScript SDKs. **200+ tests.**

## Quick start (60 seconds)

```bash
git clone https://github.com/manav8498/processfork && cd processfork
cargo build --release -p processfork
export PATH="$PWD/target/release:$PATH"

mkdir /tmp/sandbox && echo "fn main() {}" > /tmp/sandbox/main.rs
pf snapshot --agent-id demo --fs-root /tmp/sandbox
# → sha256:1c2497b0…   ⏱ 8 ms

# now edit something and snapshot again:
echo "fn main() { println!(\"hi\") }" > /tmp/sandbox/main.rs
pf snapshot --agent-id demo --fs-root /tmp/sandbox --name v2
pf log
```

The full demo (snapshot → fork ×12 → merge → push → clone on a fresh store) is **`bash demo/script.sh`**. Runs end-to-end on a laptop. No GPU, no API keys.

## When you'd reach for it

| Situation                                            | Command                                  |
|------------------------------------------------------|------------------------------------------|
| Agent about to do something destructive              | `pf snapshot pre-rm-rf`                  |
| Stuck — want to try 12 approaches in parallel        | `pf fork -n 12 --explore "fix bug"`      |
| Hand a complex session to a teammate                 | `pf push hf://you/session-name`          |
| Time-travel debug ("when did it go wrong?")          | `pf log` then `pf checkout <CID>`        |
| RL rollout fabric for agent training                 | snapshot, fan out, score, merge          |

## Use it with your stack

| Adapter | Status | What it gives you |
|---------|--------|-------------------|
| [Claude Code](./adapters/pf-claude-code/)         | ✅ ships now | `/snapshot`, `/fork`, `/merge` slash-commands inside any session |
| [LangGraph](./adapters/pf-langgraph/)             | ✅ ships now | drop-in `BaseCheckpointSaver` (full 4-layer, not just state dict) |
| [OpenInterpreter](./adapters/pf-openinterpreter/) | ✅ ships now | `interpreter.snapshot("pre-rm-rf")` then `.checkout("pre-rm-rf")` |
| [AutoGen](./adapters/pf-autogen/)                 | ✅ ships now | atomic snapshot across a whole agent group's state |
| [CrewAI](./adapters/pf-crewai/)                   | ✅ ships now | `CrewMemory` drop-in; every step time-travelable |
| [vLLM](./adapters/pf-vllm/)                       | ⏳ v1.0.1   | bit-exact KV-cache snapshot/restore (Llama-class) |
| [SGLang](./adapters/pf-sglang/)                   | ⏳ v1.0.1   | preserves `RadixAttention` prefix-sharing across restores |

## How it works

ProcessFork captures the **five things** that together make up a live agent — atomically — into one content-addressed file:

| Layer       | What it captures                                                |
|-------------|-----------------------------------------------------------------|
| **Model**   | LoRA / IA³ / full-finetune weight diffs, in-place TTT updates   |
| **Cache**   | Paged KV-cache, content-addressed per page (CoW across forks)   |
| **World**   | Filesystem, env, in-flight subprocesses, browser DOM            |
| **Effects** | Append-only ledger of irreversible tool calls (HMAC-chained)    |
| **Trace**   | Chat + tool-call message log                                    |

Identical content shares storage automatically — 12 parallel forks use **~1.5×** the space of one, not 12×. The merge engine handles each layer with the right algorithm: git-style 3-way diff for files, TIES + DARE for model weights, an HMAC chain that defends against semantic-rollback attacks (ACRFence), and an LLM-summarized "what branch B learned" patch injected into branch A's reasoning trace without re-prefilling the cache.

→ **[Architecture deep-dive](./docs/src/architecture.md)** · **[Three-way merge protocol](./docs/src/merge.md)** · **[Engineering specs](./agent_docs/)**

## Status

`v1.0.0` tagged. Numbers from `cargo bench`:

| metric                                  | observed     | target       |
|-----------------------------------------|--------------|--------------|
| Snapshot (synthetic 4-layer fixture)    | **8 ms**     | < 500 ms     |
| Cache capture, 64 pages                 | 531 µs       | —            |
| 12-fork ÷ 1-fork storage ratio          | well < 1.5×  | ≤ 1.5×       |
| Total tests passing                     | **200**      | —            |

Live KV-cache replay against a real Llama-3-8B vLLM server is the v1.0.1 deliverable — wire format and adapter API ship today, the live FFI lands next.

## Install

```bash
# From source (works today):
git clone https://github.com/manav8498/processfork && cd processfork
cargo build --release -p processfork                    # → target/release/pf

# From package registries (publishes on next release):
cargo install processfork                          # Rust CLI
pip   install processfork                          # Python SDK
npm   install @processfork/sdk                     # TypeScript SDK
```

Per-adapter packages live under `adapters/<name>/`. Full instructions in **[docs/install.md](./docs/src/install.md)**.

## Repo layout

```
crates/      Rust workspace (10 crates: pf-core, pf-cache, pf-world, pf-effects,
             pf-model, pf-merge, pf-registry, processfork (CLI, the `pf` binary), pf-py, pf-ts)
adapters/    7 first-party integration packages
benchmarks/  PFBench harness + Criterion microbench
docs/        mdBook source (25+ pages)
examples/    8 self-contained runnable examples
demo/        60-second demo recording script
```

## Docs

[Your first fork (5 min)](./docs/src/first-fork.md) · [60-second demo](./docs/src/demo.md) · [Architecture](./docs/src/architecture.md) · [Merge protocol](./docs/src/merge.md) · [Security model](./SECURITY.md) · [Performance tuning](./docs/src/tuning.md) · [Engineering specs](./agent_docs/)

## Contributing

PRs welcome. The bar is `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`, plus a green coverage delta. See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

[MIT](./LICENSE).
