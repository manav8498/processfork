# Benchmarks

> Live results: [`benchmarks/RESULTS.md`](https://github.com/processfork/processfork/blob/main/benchmarks/RESULTS.md).

Two suites:

## Microbench (build-host runnable)

`benchmarks/microbench/` — Criterion benches for the snapshot,
fork, restore, and merge hot paths. Run:

```bash
cargo bench -p pf-microbench
```

Results land under `target/criterion/`; the headline number is
`snapshot_synthetic_4layer = 7.9 ms` on macOS arm64 against the
default fixture.

## PFBench (operator-runs-it)

`benchmarks/pfbench/harness.py` — drives a model client through
SWE-Bench / GAIA / a custom 50-task long-horizon set, with two
variants per task (baseline vs ProcessFork 12-way fork-explore).
Self-test:

```bash
python benchmarks/pfbench/harness.py \
    --suite selftest --variant baseline --fork-budget 1 \
    --out /tmp/r.jsonl

python benchmarks/pfbench/aggregate.py /tmp/r.jsonl
```

Real lab runs need a model client and API keys (operator-supplied);
the harness ships an `echo` model for the self-test so CI can
verify the wiring.

## Ship gate

§M6 of the v1.0 spec asks for ProcessFork to beat the baseline by
≥15 pp pass@1 on SWE-Bench Verified. The harness emits the rows;
the lab runs and signs off.
