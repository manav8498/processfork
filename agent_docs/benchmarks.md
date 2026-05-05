# Benchmarks

Two suites:

## PFBench (macro)

`benchmarks/pfbench/` — measures whether ProcessFork actually makes agents
better at hard tasks. Three benchmarks:

1. **SWE-Bench Verified** (500 GitHub issues). Compare:
   - Vanilla GPT-4o (baseline)
   - Vanilla GPT-4o + ProcessFork (12-way fork-explore at every "I'm stuck"
     decision point)
   - **Ship gate: ProcessFork variant beats baseline by ≥15 pp pass@1.**

2. **GAIA** (Level-1, Level-2). Same comparison.

3. **PF-LongHorizon-50** — a custom 50-task set we author covering web
   research → code → deploy → verify pipelines (multi-day equivalents).

The harness lives in `benchmarks/pfbench/harness.py`; results go in
`benchmarks/RESULTS.md` with reproducible scripts and seed-pinned configs.

PFBench requires API keys (OpenAI / Anthropic) and is operator-run; the
harness is the deliverable.

## microbench (micro)

`benchmarks/microbench/` — Criterion benches measuring:

| metric                              | budget                |
|-------------------------------------|-----------------------|
| `snapshot_synthetic_4layer`         | <500 ms (proxy fixt.) |
| `fork_n12_metadata_only`            | <100 ms each          |
| `restore_1_2gb_cold_cache`          | <5 s                  |
| `cas_dedup_12_forks_storage_ratio`  | ≤1.5×                 |
| `merge_three_way_synthetic`         | <50 ms                |

Run: `cargo bench --workspace`. Results saved to `target/criterion/` and
summarized into `benchmarks/microbench/RESULTS.md` by the gate script.

The synthetic 4-layer fixture is a representative workload that runs on the
build host (macOS arm64), not the H100. Real-hardware figures get appended
when the operator runs the GPU-gated suite.
