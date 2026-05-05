# Feature spec — v1.0.0 ship checklist

The literal ship checklist. Mirror this into release-checklist.md before tag.
Every box must be checkable against a real artifact, not a unit test.

## M1 — Core capability

- [ ] `pf snapshot <agent-id>` captures all four layers atomically into one
      `.pfimg` artifact in <500 ms p99 for a 380K-token Llama-3-70B agent on a
      Hopper-class GPU. (Real-hardware: gated `$PF_HAS_GPU=1`. Synthetic
      fixture proxy: <500 ms on macOS arm64 in CI.)
- [ ] `pf fork <CID> -n 12` spawns 12 divergent live branches in <100 ms p99
      each, copy-on-write on every layer.
- [ ] `pf checkout <CID>` restores a snapshot bit-exact on a different
      machine of the same architecture in <5 s for a 1.2 GB image.
- [ ] `pf merge <branch> -> <main>` performs typed, effect-aware three-way
      merge with explicit conflict surfacing.
- [ ] Bit-exact replay verified by `tests/bit_exact_replay.rs`: snapshot mid-
      tool-call, restore on clean machine, run 100 more tokens, assert logit-
      identical (within batch-invariant tolerance) against the original.

## M2 — Layered architecture

- [ ] **Model**: weight-diff capture for LoRA, IA³, full-finetune, In-Place
      TTT. TIES + DARE merge tested vs. mergekit reference outputs.
- [ ] **Cache**: paged KV-cache, content-addressed pages, CoW across forks;
      vLLM ≥0.10 + SGLang ≥0.5 adapters in batch-invariant mode.
- [ ] **World**: overlayfs (Linux) + APFS clone (macOS); env vars; CRIU dump
      for in-flight subprocesses (Linux); CDP DOM dump (Playwright/Puppeteer).
- [ ] **Effects**: append-only ledger, per-call idempotency keys, per-tool
      side-effect class, replay-or-fork enforced by ACRFence-aware policy.

## M3 — Distribution surfaces

- [ ] `pf` CLI: single static binary <15 MB stripped, all 12 subcommands.
- [ ] Python SDK on PyPI (`pip install processfork`).
- [ ] TypeScript SDK on npm (`npm install @processfork/sdk`).
- [ ] Rust crate on crates.io (`cargo add processfork`).
- [ ] Docker image on GHCR (`ghcr.io/manav8498/processfork:1.0.0`).

## M4 — Registry adapters

- [ ] Hugging Face Hub (`hf://user/repo`).
- [ ] S3-compatible (R2 / MinIO / AWS S3).
- [ ] IPFS (feature-flag gated).
- [ ] Local OCI registry (air-gapped).

## M5 — Integration adapters (each with end-to-end example)

- [ ] Claude Code wrapper (`pf wrap claude`, `/snapshot /fork /merge`).
- [ ] LangGraph (replaces checkpointer with full four-layer ProcessFork).
- [ ] OpenInterpreter.
- [ ] vLLM native server plugin.
- [ ] SGLang native server plugin.
- [ ] AutoGen.
- [ ] CrewAI.

## M6 — Benchmarks

- [ ] PFBench: SWE-Bench Verified + GAIA + 50-task long-horizon set.
- [ ] GPT-4o + ProcessFork beats GPT-4o by ≥15 pp on SWE-Bench Verified
      (operator-run; agent ships harness + reproducible script).
- [ ] microbench: snapshot, restore, fork overhead, storage efficiency,
      merge correctness (proptest on synthetic divergent histories).
- [ ] `benchmarks/RESULTS.md` published with reproducible scripts.

## M7 — Documentation

- [ ] README with the 60-second viral demo at the top.
- [ ] mdBook docs site, deployed to GitHub Pages.
- [ ] `docs/architecture.md` deep-dive of all four layers + merge protocol.
- [ ] Auto-generated API reference (Rust + Python + TypeScript).
- [ ] "Your first agent fork" 5-minute tutorial.
- [ ] One migration guide per integration target (7).
- [ ] Performance tuning guide.
- [ ] CONTRIBUTING.md, CODE_OF_CONDUCT.md, SECURITY.md.

## M8 — Tests & CI

- [ ] Rust unit tests, ≥85 % line coverage (cargo llvm-cov).
- [ ] Property tests (proptest) for merge: commutativity-where-applicable,
      idempotency, no-effect-replay invariant.
- [ ] Integration tests for all 7 adapters against real local Llama-3-8B
      (Llama-3-70B optional, gated).
- [ ] Bit-exact reproducibility test across two machines of same arch.
- [ ] GitHub Actions CI: ubuntu-24.04 (x86_64 + arm64) + macos-14 (arm64).
- [ ] Release automation: signed binaries → GitHub Releases + PyPI + npm +
      crates.io + GHCR on tag.

## M9 — Demo & launch readiness

- [ ] 8 examples in `examples/`:
      single-fork, 12-way parallel, cross-machine portable agent, time-
      travel debugging, RL rollout fabric, SWE-Bench fork-explore, Claude
      Code session fork, browser-agent DOM fork.
- [ ] `demo/script.cast` for the 60-second viral video.
- [ ] Working `cargo install processfork`, `pip install processfork`,
      `npm install @processfork/sdk` from registry.
- [ ] Tagged `v1.0.0` GitHub release with signed notes.

## What "done" means

No partial credit. A "ProcessFork that snapshots only the cache" is not v1.0.
Ship all four layers or do not ship.
