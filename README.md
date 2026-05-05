# ProcessFork

> **`fork()` for AI agents.** Snapshot a live agent — model fast-weights,
> KV-cache, sandbox filesystem, tool-effect ledger, and reasoning trace —
> into one content-addressed image. Branch it. Merge it. Push it. Replay it.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Status: v1.0-rc](https://img.shields.io/badge/status-v1.0--rc-orange.svg)](#status)
[![Tests: 200+ passing](https://img.shields.io/badge/tests-200%2B%20passing-brightgreen.svg)](#tests)

## The 60-second demo

```bash
# 1. Snapshot a live agent state
$ pf snapshot --agent-id claude-code --fs-root .
sha256:1c2497b0dc23d21b8068b26f54c0d8b14b7fdf704c11a456dca7e36eaf6fbed6
                                                                    ⋮ 8 ms
# 2. Fork into 12 divergent branches
$ pf fork sha256:1c24… -n 12 --explore "fix the type error"
sha256:3ca3a0ff…
sha256:67711987…           ⋮ 12 lines, ~10 ms each (CoW; manifest-level)
sha256:2db9ff46…

# 3. After exploring, merge the winner back
$ pf merge sha256:842640c0… --into sha256:1c24…
  ancestor : sha256:1c24…
  trace    : clean (47 chars summary; ~12 re-prefill tokens)
  world    : clean (1 clean paths; 0 conflicts)
  effects  : merged blob = sha256:52e0c81c…
  model    : applied TIES + DARE
merged   : sha256:a41afac1…

# 4. Ship to a registry; teammate clones on a different machine
$ pf push sha256:a41afac1… file:///mnt/registry         # or hf:// / s3:// / oci://
$ pf --store /other/store clone file:///mnt/registry --into /tmp/restored
sha256:a41afac1…
✓ restored to /tmp/restored
```

This entire transcript runs end-to-end on a laptop today (no GPU
required). Run it: `bash examples/02-cli-snapshot/run.sh`.

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
See [`docs/`](./docs/src/architecture.md) for the architecture
deep-dive.

## Install

```bash
cargo install processfork           # Rust CLI
pip   install processfork           # Python SDK
npm   install @processfork/sdk      # TypeScript SDK
```

(Operator-side until v1.0.0 publishes; build-from-source instructions
live in [`docs/src/install.md`](./docs/src/install.md).)

## Hello, fork (Python)

```python
import processfork as pf

store = pf.PfStore.open("~/.processfork")
cid = pf.snapshot_filesystem(
    store,
    agent_kind="claude-code",
    fs_root="/tmp/sandbox",
    env={"PWD": "/tmp/sandbox"},
    messages=[{"role": "user", "content": "go"}],
)
report = pf.merge(store, cid, cid)   # idempotent self-merge → "clean"
print(report["overall"])
```

## Repository layout

```
crates/      Rust workspace (pf-core, pf-cache, pf-world, …, pf-cli, pf-py, pf-ts)
adapters/    7 per-framework integration packages (Claude Code, LangGraph, vLLM, …)
benchmarks/  PFBench harness + Criterion microbenches
docs/        mdBook source
examples/    8 self-contained runnable examples
landing/     GitHub Pages site
demo/        60-second demo recording scripts
agent_docs/  Subsystem specifications (the engineering source of truth)
.claude/     Sub-agents, skills, hooks for the build agent
```

## Tests

200+ tests pass on macOS arm64 (latest tag: [`phase-10-complete`](#)):

| surface             | tests                                  |
|---------------------|----------------------------------------|
| Rust workspace      | 154 unit + integration + doc           |
| Python SDK smoke    | 5  (pyo3 + maturin wheel)              |
| TypeScript SDK smoke| 5  (napi-rs + node --test)             |
| Adapter smoke       | 36 (3 fully wired + 4 scaffolded)      |
| GPU-gated skips     | 2  (vLLM, SGLang bit-exact)            |

Latest microbench (build host, no GPU): `snapshot_synthetic_4layer
= 7.9 ms` against the 500 ms budget — ~63× headroom.

## Building from source

```bash
cargo check --workspace
cargo test  --workspace
```

## Status

v1.0 is feature-complete on this build host. Operator-supplied
deliverables (PyPI / npm / crates.io publishes, real-hardware GPU
bit-exact replay, live HF / S3 push) are tracked in
[`agent_docs/release-checklist.md`](./agent_docs/release-checklist.md).
What's deferred to v1.0.1 / v1.1 (vLLM live FFI, cosign keyless,
browser DOM capture) is documented in each adapter's README.

## License

MIT. See [LICENSE](LICENSE).
