<h1 align="center">ProcessFork</h1>
<p align="center"><b><code>git</code> for AI agents.</b> Snapshot, fork, and merge live LLM sessions in <b>8&nbsp;ms</b>.</p>

<p align="center">
  <img src=".github/hero.svg" alt="snapshot a 4-hour Claude Code session in 8 ms, fork into 12 attempts, merge the winner back, push to a registry" width="100%">
</p>

<p align="center">
  <a href="./demo/processfork-demo.cast">
    <img src="./demo/processfork-demo.gif" alt="60-second demo: pf snapshot → pf fork ×12 → pf merge → pf push file:// → pf clone on a fresh store" width="100%">
  </a>
  <br>
  <sub>↑ <a href="./demo/processfork-demo.cast">Replay it locally</a>: <code>asciinema play demo/processfork-demo.cast</code></sub>
</p>

<p align="center">
  <a href="https://crates.io/crates/processfork"><img src="https://img.shields.io/crates/v/processfork?label=crates.io&color=orange" alt="crates.io"></a>
  <a href="https://pypi.org/project/processfork/"><img src="https://img.shields.io/pypi/v/processfork?label=PyPI&color=blue" alt="PyPI"></a>
  <a href="https://www.npmjs.com/package/@processfork/sdk"><img src="https://img.shields.io/npm/v/@processfork/sdk?label=npm&color=red" alt="npm"></a>
  <a href="https://github.com/manav8498/processfork/releases/latest"><img src="https://img.shields.io/github/v/release/manav8498/processfork?label=release" alt="release"></a>
  <a href="https://github.com/manav8498/processfork/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT"></a>
</p>
<p align="center">
  <a href="https://github.com/manav8498/processfork/actions"><img src="https://img.shields.io/github/actions/workflow/status/manav8498/processfork/ci.yml?branch=main&label=CI" alt="CI"></a>
  <a href="#status"><img src="https://img.shields.io/badge/tests-200%20passing-brightgreen" alt="200 tests"></a>
  <a href="#status"><img src="https://img.shields.io/badge/snapshot-8%20ms-brightgreen" alt="8 ms snapshot"></a>
  <a href="./coverage/README.md"><img src="https://img.shields.io/badge/coverage-88.96%25-brightgreen" alt="88.96% line coverage"></a>
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
# install the CLI:
cargo install processfork                      # → `pf` on your $PATH

# snapshot a directory:
mkdir /tmp/sandbox && echo "fn main() {}" > /tmp/sandbox/main.rs
pf snapshot --agent-id demo --fs-root /tmp/sandbox
# → sha256:1c2497b0…   ⏱ 8 ms

# edit something, snapshot again, see the diff:
echo "fn main() { println!(\"hi\") }" > /tmp/sandbox/main.rs
pf snapshot --agent-id demo --fs-root /tmp/sandbox --name v2
pf log
pf diff <first-cid> <second-cid>
```

Prefer Python? `pip install processfork`. TypeScript? `npm install @processfork/sdk`.

The **full 60-second demo** (snapshot → fork ×12 → merge → push → clone on a fresh store) is `bash demo/script.sh`. Runs end-to-end on a laptop. No GPU, no API keys.

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
| [Claude Code](./adapters/pf-claude-code/)         | ✅ ships v1.0 | `/snapshot`, `/fork`, `/merge` slash-commands inside any session |
| [LangGraph](./adapters/pf-langgraph/)             | ✅ ships v1.0 | drop-in `BaseCheckpointSaver` over the FS+env+trace+effects layers |
| [OpenInterpreter](./adapters/pf-openinterpreter/) | ✅ ships v1.0 | `interpreter.snapshot("pre-rm-rf")` then `.checkout("pre-rm-rf")` |
| [AutoGen](./adapters/pf-autogen/)                 | ✅ ships v1.0 | atomic FS+env+trace+effects snapshot across an agent group |
| [CrewAI](./adapters/pf-crewai/)                   | ✅ ships v1.0 | `CrewMemory` drop-in; every step time-travelable |
| [vLLM](./adapters/pf-vllm/)                       | 🟡 mock ships v1.0 · live = Modal lane | mock: K/V page bytes + manifest persist & restore via the SDK; live (Modal A10G): V0 engine bit-exact, V1 engine output-equivalent (see "What does/doesn't ship" below) |
| [SGLang](./adapters/pf-sglang/)                   | 🟡 mock ships v1.0 · live = Modal lane | mock: RadixCache `k_buffer`/`v_buffer` page round-trip; live: scaffolded — Modal lane reaches the parity stub but full radix-tree replay is v1.1 |

## How it works

ProcessFork captures the **five things** that together make up a live agent — atomically — into one content-addressed file. Each layer ships at a different maturity level in v1.0.x:

| Layer       | What it captures                                                | v1.0.x status |
|-------------|-----------------------------------------------------------------|---------------|
| **World**   | Filesystem (full), env (default-redacted), browser DOM (CDP). In-flight subprocesses **are not** captured by `pf snapshot` — the `procs` blob writes a `procs.unsupported.v1` placeholder unless a CRIU/zombie-restart adapter is wired in. | ✅ FS + env ship; procs = placeholder |
| **Effects** | Append-only ledger of tool calls, HMAC-chained per entry (ACRFence). | ✅ ships (CLI + Python SDK + TS SDK + 5 adapters) |
| **Trace**   | Chat + tool-call message log                                    | ✅ ships |
| **Model**   | LoRA / IA³ / full-finetune weight diffs, in-place TTT updates. The format and TIES+DARE merge math ship and are exercised on the Modal A10G lane; the **generic CLI snapshot path produces an empty LoRA envelope** because the layer is populated by adapters (vLLM/SGLang/etc.), not by walking a directory. | 🟡 format ships; CLI path is placeholder; adapter-populated |
| **Cache**   | Paged KV-cache, content-addressed per page (CoW across forks). Same shape: format + page math ship; the **generic CLI snapshot produces an empty page manifest**; the vLLM/SGLang adapters populate it for real. | 🟡 format ships; CLI path is placeholder; adapter-populated |

Identical content shares storage automatically — 12 parallel forks use **~1.004×** the space of one in the operator's matrix run, well under the < 1.5× budget. The merge engine handles each layer with the right algorithm: git-style 3-way diff for files (conflict markers materialize; resolution UI is v1.1), TIES + DARE for model weights, the HMAC effects chain that defends against semantic-rollback attacks (ACRFence), and an LLM-summarized "what branch B learned" patch injected into branch A's reasoning trace without re-prefilling the cache.

### What does and doesn't ship in v1.0.x

**Production-credible today** (independent retest, 12/12 matrix passing):

- `pf snapshot` / `pf checkout` for filesystem sandboxes, with default secret-shaped env redaction.
- HMAC-chained effects ledger end-to-end (CLI + Python + TS), tamper detected by `pf verify`.
- Fork & merge: 12 forks at ~1.004× storage; clean and conflicting merges produce content-addressed merged CIDs with Git-style markers in conflict files.
- File:// (and OCI / S3 / HF) registry transport.
- 5 adapters (Claude Code, LangGraph, OpenInterpreter, AutoGen, CrewAI) over the FS + env + trace + effects layers.
- vLLM/SGLang **mock** mode: K/V page bytes + manifest persist into the store and read back on checkout.

**Not yet production-ready, though the format and code paths exist**:

- **Live in-flight subprocess capture**. The world layer's `procs` blob is a placeholder (`procs.unsupported.v1`); a CRIU-based adapter is the v1.1 deliverable. Today, restored sessions do not bring back live PIDs; they bring back the FS + env + trace + effects state that lets a fresh worker continue.
- **Local PF_HAS_GPU=1 vLLM/SGLang test** (`examples/06`, `examples/07`, `pf-cache/tests/cache_bit_exact_vllm.rs`). These exit 2 with a "use the adapter packages directly + Modal lane" pointer — they were operator-runs-it skeletons that never got a self-contained subprocess flow. The Modal A10G lane (`scripts/gpu-validate-modal.py`) **does** run vLLM end-to-end and emits the JSONs in [`benchmarks/gpu-validation/`](./benchmarks/gpu-validation/).
- **Bit-exact KV-cache restore on vLLM V1 engine.** The Modal lane shows V0 engine **`bit_exact: true`** for 38 619 KV pages but **V1 engine = output-equivalent (first-80-chars match)**, not bit-exact, on TinyLlama-1.1B. V1 is using `collective_rpc` and the engine has its own non-determinism in deterministic mode that we do not yet eliminate. Treat live V1 KV restore as "lossy semantic restore" today.
- **Conflict-merge resolution UI.** The merge engine writes Git-style `<<<<<<<` markers and emits a merged CID; an interactive `pf merge --resolve <cid>` flow is v1.1.
- **Generic CLI model/cache layer capture.** The generic `pf snapshot` produces empty model + cache envelopes — these layers are populated through adapters, not by walking a directory. If you want the model+cache layers populated, use the vLLM or SGLang adapter from inside your engine process.

→ **[Architecture deep-dive](./docs/src/architecture.md)** · **[Three-way merge protocol](./docs/src/merge.md)** · **[Engineering specs](./agent_docs/)**

## Status

`v1.0.11` tagged. Documentation honesty pass after the v1.0.10 retest: the README's "ships now" framing on vLLM/SGLang and the "bit-exact" metric row were not telling the same story as `benchmarks/gpu-validation/*.json` and the `examples/06`+`07` runners that exit 2 under `PF_HAS_GPU=1`. This release does not change runtime behavior — the v1.0.10 fixes (TS SDK scrub + HMAC ledger), v1.0.9 fixes (Python SDK scrub + HMAC ledger), and earlier audit-round fixes all stand. What it changes: the adapter status table separates **mock** from **live (Modal lane)**; the 5-layer table marks **Model** and **Cache** as adapter-populated (the generic CLI path emits empty envelopes); a new "What does and doesn't ship in v1.0.x" subsection makes the boundary explicit (no in-flight subprocess capture; no local PF_HAS_GPU=1 self-contained vLLM test; no V1-engine bit-exactness; no conflict-resolution UI). The example runners and the `cache_bit_exact_vllm.rs` panic message are also updated to point at the actually-true status. `cargo deny check`: still `advisories ok, bans ok, licenses ok, sources ok`.

| metric                                                 | observed                 | target           |
|--------------------------------------------------------|--------------------------|------------------|
| Snapshot p50, synthetic 4-layer fixture (macOS arm64)  | **7.9 ms**               | < 500 ms p99     |
| Snapshot p50, real GPU host (Modal A10G, 64 × 4 KiB)   | **42 ms** (warm)         | < 500 ms p99     |
| KV-cache restore, **vLLM V0 engine** + TinyLlama-1.1B on A10G | **`bit_exact: true`** — 38 619 KV pages, regenerated text byte-identical ([JSON](./benchmarks/gpu-validation/2026-05-06-modal-a10g.json)) | `out_a == out_b` byte-equal |
| KV-cache restore, **vLLM V1 engine** (`collective_rpc`) | **output-equivalent, not bit-exact** — first-80-chars match across snapshot/restore on 38 599 KV pages ([JSON](./benchmarks/gpu-validation/2026-05-06-modal-a10g-vllm-v1.json)); `bit_exact: false` field is the source of truth | `out_a == out_b` byte-equal (target unmet on V1) |
| Cache capture, 64 pages                                | 531 µs                   | —                |
| 12-fork ÷ 1-fork storage ratio (auditor's matrix)      | **1.004×**               | ≤ 1.5×           |
| Total Rust tests passing                               | **199**                  | —                |
| Python SDK + Claude adapter tests                      | **17**                   | —                |
| TS SDK smoke tests                                     | **8** (incl. 3 v1.0.10 regressions) | —     |

Synthetic-fixture numbers come from `cargo bench --workspace`. GPU numbers come from `modal run scripts/gpu-validate-modal.py`; raw JSON lives in [`benchmarks/gpu-validation/`](./benchmarks/gpu-validation/) and the breakdown in [`benchmarks/RESULTS.md`](./benchmarks/RESULTS.md). The local PF_HAS_GPU=1 paths in `examples/06` and `examples/07` are not the validation path — they exit 2 with a Modal-lane pointer; the validation IS the Modal lane, and the JSONs above are its output.

## Install

```bash
cargo install processfork                          # Rust CLI (the `pf` binary)
pip   install processfork                          # Python SDK
npm   install @processfork/sdk                     # TypeScript SDK
```

**Per-adapter packages** (one each on PyPI):

```bash
pip install processfork-claude-code
pip install processfork-langgraph
pip install processfork-openinterpreter
pip install "processfork-vllm[vllm]"               # needs CUDA + vllm ≥ 0.10
pip install "processfork-sglang[sglang]"           # needs CUDA + sglang ≥ 0.5
pip install "processfork-autogen[autogen]"
pip install "processfork-crewai[crewai]"
```

**Build from source** if you want to hack on it:

```bash
git clone https://github.com/manav8498/processfork && cd processfork
cargo build --release -p processfork               # → target/release/pf
```

Full build-from-source instructions in **[docs/install.md](./docs/src/install.md)**. Pre-built wheels cover **macOS arm64**, **Linux x86_64**, and **Linux aarch64**; macOS Intel + Windows wheels arrive in v1.0.1 (operator: same package, just more platforms).

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
