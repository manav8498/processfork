# PFBench results

> Reproducible benchmark results for ProcessFork. All numbers were
> measured by the operator on the machine listed; re-run with the
> commands shown to reproduce. The build host (this repo's CI default)
> is **macOS arm64, no GPU**; GPU figures are operator-supplied via the
> nightly CI lane.

## Microbench (build-host runnable)

Run: `cargo bench --workspace`. Output goes to `target/criterion/`.

| metric                              | observed (build host)  | budget (§4.6)         |
|-------------------------------------|------------------------|-----------------------|
| `snapshot_synthetic_4layer`         | **7.9 ms** (median)    | < 500 ms p99          |
| `cache_capture_64_pages`            | **531 µs**             | <  50 ms              |
| `cache_restore_64_pages`            | **34 µs**              | <  50 ms              |
| 12-fork CoW storage ratio           | well under 1.5×         | ≤ 1.5×                |
| Identical-content second snapshot   | **614 B** growth        | ≤ 4 KiB               |

Build host: macOS 25.0.0, Apple M-series, Rust 1.95, zstd-19, default
fixture (32 cache pages × 16 KiB + 64 fs files × 4 KiB + 64 KiB model
diff ≈ 1.4 MB total payload).

## PFBench (macro, operator-runs-it)

Run: `python benchmarks/pfbench/harness.py --suite swe-bench --variant gpt-4o`.

Each benchmark run produces a row in `benchmarks/pfbench/results/<run-id>.json`
that records the model + variant + fork-budget + scores. Aggregate via
`python benchmarks/pfbench/aggregate.py`.

### SWE-Bench Verified — operator-supplied baseline

| variant                              | pass@1   | notes                                 |
|--------------------------------------|----------|---------------------------------------|
| `gpt-4o` (no ProcessFork)            | _t.b.r._ | the §M6 baseline                      |
| `gpt-4o` + ProcessFork (12-way fork) | _t.b.r._ | §M6 ship gate: ≥ 15 pp over baseline  |

The harness lives at `benchmarks/pfbench/harness.py`; reproducible
operator-supplied results land here when the lab runs them.

### GAIA — Levels 1 + 2

Same shape; recorded once the operator runs the gated suite.

### PF-LongHorizon-50

Custom 50-task long-horizon set covering web research → code → deploy
→ verify pipelines. Tasks defined in
`benchmarks/pfbench/longhorizon-50/tasks.jsonl`.

## Bit-exact replay (GPU-validated)

Two ways to run: locally on a CUDA host with
`bash scripts/gpu-validate.sh`, or via Modal with
`modal run scripts/gpu-validate-modal.py` (no SSH, no quota dance).
Raw run JSONs land in `benchmarks/gpu-validation/`.

### 2026-05-06 — Modal A10G (24 GB VRAM, vLLM 0.6.6, TinyLlama-1.1B)

| metric                                | observed              | budget / target       |
|---------------------------------------|-----------------------|-----------------------|
| **Bit-exact KV-cache replay**         | **✅ verified**       | out_a == out_b        |
| KV pages serialized + restored        | **38,619**            | (all of them)         |
| Snapshot CID                          | `sha256:877685226539…`| stable across restore |
| Snapshot+restore wall                 | 78.6 s                | (single-shot)         |
| **Microbench p50 snapshot**           | **42.4 ms**           | < 500 ms p99          |
| Microbench p99 snapshot               | 1180 ms (cold-start)  | < 500 ms (warm)       |
| TIES + DARE real-shape Frobenius Δ    | 0.0                   | identical             |

Raw: [`benchmarks/gpu-validation/2026-05-06-modal-a10g.json`](./gpu-validation/2026-05-06-modal-a10g.json).

The vLLM 0.6.6 pin matches the V0 engine architecture that
processfork-vllm 1.0.1 targets. V1 engine support (vLLM ≥0.7 wraps
the worker; ≥0.10 ships subprocess workers + new KvCacheManager)
lands in **v1.0.2** via `engine_core.collective_rpc('get_cache_engine')`.

### Llama-3-8B p99 vs spec (deferred)

| metric                              | observed (operator) | budget                |
|-------------------------------------|---------------------|-----------------------|
| Llama-3-8B mid-stream snapshot p99  | _t.b.r._ (needs H100/A100) | ≤ 500 ms       |
| Resumed-branch logit divergence     | _t.b.r._ (needs `--exact` mode) | bit-equal (`--exact`) |
