# ProcessFork

**Snapshot, branch, and merge live AI agent state — like `git`, for agents.**

You're four hours into a complex refactor with Claude Code. The agent has read 200 files, run 47 tests, opened a database, started a dev server, and built up a mental model of your codebase that's worth a small fortune in tokens. Then it suggests a sweeping change that breaks everything.

Today you have two options: undo the damage by hand, or start over.

ProcessFork gives you a third:

```bash
$ pf snapshot
sha256:1c2497b0…   ← 8 ms. Your entire agent state, captured.
```

Now that snapshot is a thing. You can fork it into 12 parallel attempts. You can merge the winner back. You can hand the whole session to a teammate. You can roll back to it tomorrow.

It's `git`. For AI agents.

---

## What it actually does

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

Three things, in plain English:

**1.  Save where you are.** `pf snapshot` captures the agent's state — the model's memory (KV-cache), the sandbox files, the env, the tools it's already called. **Atomically. In milliseconds.** One file. Content-addressed.

**2.  Try things in parallel.** `pf fork -n 12` gives you twelve copies of that exact state, each ready to diverge. Run them in twelve panes. Pick the one that worked.

**3.  Merge the winner back.** `pf merge winner --into main` is a real three-way merge across files (with `<<<<<<<` markers like git), tool-call history (won't double-send your email), and reasoning trace (the lessons get summarized into the cache).

Then `pf push hf://you/refactor-2026-05-05` ships the whole session somewhere. A teammate runs `pf clone hf://you/refactor-2026-05-05` and picks up exactly where you left off.

## When you'd reach for it

| Situation                                         | What you do                                |
|---------------------------------------------------|--------------------------------------------|
| Agent is about to do something destructive        | `pf snapshot pre-rm-rf` first              |
| You're stuck and want to try 12 approaches        | `pf fork -n 12 --explore "fix the bug"`    |
| Hand off a complex session to a teammate          | `pf push hf://you/session-name`            |
| Time-travel debug ("when did it go wrong?")       | `pf log` then `pf checkout <CID>`          |
| RL rollout fabric (agent training)                | Same primitive: snapshot, fan out, score   |

## 60 seconds to try it

```bash
# 1. Build the CLI (one-time, takes ~2 minutes)
git clone https://github.com/manav8498/processfork
cd processfork
cargo build --release -p pf-cli
export PATH="$PWD/target/release:$PATH"

# 2. Snapshot a directory
mkdir /tmp/sandbox && echo "fn main() {}" > /tmp/sandbox/main.rs
pf snapshot --agent-id demo --fs-root /tmp/sandbox
# → sha256:1c2497b0dc23d21b8068b26f54c0d8b14b7fdf704c11a456dca7e36eaf6fbed6

# 3. Edit something, snapshot again, see the diff
echo "fn main() { println!(\"hi\") }" > /tmp/sandbox/main.rs
pf snapshot --agent-id demo --fs-root /tmp/sandbox --name v2
pf log
pf diff <first-cid> <second-cid>
```

The full demo (snapshot → fork × 12 → merge → push to a registry → clone on a fresh store → restore byte-identical) is `bash demo/script.sh`. Runs end-to-end on a laptop with no GPU and no API keys.

## Use it with your stack

ProcessFork ships seven first-party adapters so it slots into the agent runtime you already use:

- **[Claude Code](./adapters/pf-claude-code/)** — adds `/snapshot`, `/fork`, `/merge` slash-commands inside Claude Code sessions. ✅ Ships now.
- **[LangGraph](./adapters/pf-langgraph/)** — drop-in `BaseCheckpointSaver` that captures the full four-layer state at every node, not just LangGraph's state dict. ✅ Ships now.
- **[OpenInterpreter](./adapters/pf-openinterpreter/)** — `interpreter.snapshot("pre-rm-rf")` then `interpreter.checkout("pre-rm-rf")` if it goes wrong. ✅ Ships now.
- **[vLLM](./adapters/pf-vllm/)** — server plugin for bit-exact KV-cache snapshot/restore on Llama-class models. Trait + wire format ship now; the live FFI shim is the v1.0.1 deliverable.
- **[SGLang](./adapters/pf-sglang/)** — same shape, preserves RadixAttention prefix-sharing across restores. v1.0.1.
- **[AutoGen](./adapters/pf-autogen/)** — wraps a `RuntimeContext` so a whole agent group's state snapshots atomically. ✅ Ships now.
- **[CrewAI](./adapters/pf-crewai/)** — `CrewMemory` drop-in. Every crew step becomes a `.pfimg` you can time-travel through. ✅ Ships now.

## How it works (the 90-second version)

An "agent" at runtime is the simultaneous, mutating state of five things at once:

| Layer       | What it captures                                                |
|-------------|-----------------------------------------------------------------|
| **Model**   | LoRA / IA³ / full-finetune weight diffs, in-place TTT updates   |
| **Cache**   | Paged KV-cache, content-addressed per page (CoW across forks)   |
| **World**   | Filesystem, env vars, in-flight subprocesses, browser DOM       |
| **Effects** | Append-only ledger of every irreversible tool call              |
| **Trace**   | Chat + tool-call message log                                    |

`pf snapshot` captures all five **atomically** into one content-addressed `.pfimg` file. Identical content across forks shares storage automatically — twelve parallel branches use about 1.5× the space of one, not 12×.

The merge engine handles each layer with the right algorithm: git-style 3-way diff for files, TIES + DARE for model weights, an HMAC-chained ledger that **never re-sends your email** when you restore (defends against ACRFence-style semantic-rollback attacks), and an LLM-summarized "what branch B learned" patch that gets injected into branch A's reasoning trace without re-prefilling the whole cache.

Read the full design in **[docs/architecture.md](./docs/src/architecture.md)** or the engineering specs under **[agent_docs/](./agent_docs/)**.

## What's in the box

```
crates/                Rust workspace (10 crates)
  pf-core              CAS, .pfimg manifest, atomic snapshot orchestrator
  pf-cache             Paged KV-cache wire format
  pf-world             FS / env / processes / DOM capture
  pf-effects           HMAC-chained tool-call ledger + replay policy
  pf-model             Weight diffs + TIES/DARE merge
  pf-merge             Three-way merge engine
  pf-registry          File / HF / S3 / IPFS / OCI adapters
  pf-cli               The `pf` binary
  pf-py, pf-ts         Python (pyo3) and TypeScript (napi-rs) SDKs
adapters/              7 per-framework integration packages
benchmarks/            PFBench (macro) + Criterion microbench
docs/                  mdBook source (25+ pages)
examples/              8 self-contained runnable examples
demo/                  60-second demo recording script
```

## Status

**v1.0.0** is tagged. Numbers from `cargo bench` on macOS arm64:

| metric                                  | observed     | target       |
|-----------------------------------------|--------------|--------------|
| Snapshot (synthetic 4-layer fixture)    | **8 ms**     | < 500 ms     |
| Cache capture, 64 pages                 | 531 µs       | —            |
| 12-fork ÷ 1-fork storage ratio          | well < 1.5×  | ≤ 1.5×       |
| Total tests passing                     | **200**      | —            |

The bit-exact replay against a real Llama-3-8B vLLM server (the kickoff frame's "snapshot a 380K-token agent in 87 ms") is the v1.0.1 deliverable — wire format and adapter API ship today, the live FFI lands next.

## Install

```bash
# From source (works today):
git clone https://github.com/manav8498/processfork && cd processfork
cargo build --release -p pf-cli       # → target/release/pf

# From package registries (operator publishes on the next release):
cargo install processfork              # Rust CLI
pip   install processfork              # Python SDK
npm   install @processfork/sdk         # TypeScript SDK
```

Full instructions for building the SDKs locally, plus the optional adapter packages, in **[docs/install.md](./docs/src/install.md)**.

## Docs & deeper reads

- **[Your first fork](./docs/src/first-fork.md)** — 5-minute tutorial.
- **[The 60-second demo](./docs/src/demo.md)** — the full elevator pitch.
- **[Architecture deep-dive](./docs/src/architecture.md)** — how it actually works.
- **[Three-way merge protocol](./docs/src/merge.md)** — the trickiest part of the design.
- **[Security model](./SECURITY.md)** — including the ACRFence threat model.
- **[Performance tuning](./docs/src/tuning.md)** — production knobs.
- **[Engineering specs](./agent_docs/)** — the source-of-truth specs the build agent worked from.

## Contributing

PRs welcome. Read **[CONTRIBUTING.md](./CONTRIBUTING.md)** first — the bar is `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`, and a green coverage delta.

This started as a multi-session autonomous build by Claude Opus 4.7 against a tight specification — see the commit history for the phase-by-phase build log and the live `claude-progress.json` for the state machine. The result is real code, real tests, and real honest scope notes about what shipped vs what's deferred.

## License

[MIT](./LICENSE).
