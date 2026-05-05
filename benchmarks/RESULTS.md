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

## Bit-exact replay (GPU-gated)

Run: `PF_HAS_GPU=1 cargo test --test cache_bit_exact_vllm` and
`PF_HAS_GPU=1 PYTHONPATH=adapters/pf-vllm pytest adapters/pf-vllm/tests/`.

| metric                              | observed (operator) | budget                |
|-------------------------------------|---------------------|-----------------------|
| Llama-3-8B mid-stream snapshot p99  | _t.b.r._            | ≤ 500 ms              |
| Resumed-branch logit divergence     | _t.b.r._            | bit-equal (`--exact`) |
