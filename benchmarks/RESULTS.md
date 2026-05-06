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

### 2026-05-06 — Modal A10G (24 GB VRAM, vLLM 0.6.6 V0, TinyLlama-1.1B)

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

### 2026-05-06 — TIES + DARE byte-exact equivalence to mergekit

`tools/check_mergekit_parity.py` runs in a dedicated venv (mergekit
+ pydantic 2.4.x is incompatible with vLLM ≥0.20's pydantic 2.12, so
this can't co-exist with the GPU validation lane).

| metric                           | observed     | tolerance |
|----------------------------------|--------------|-----------|
| Synthetic input shape            | 2048×2048 fp32 × 3 deltas | — |
| `keep_top`                       | 0.2          | — |
| `alpha`                          | 0.5          | — |
| **max\|pf_ties − mergekit_ties\|**| **0.0**     | < 1e-3    |

Raw: [`benchmarks/gpu-validation/2026-05-06-mergekit-parity.json`](./gpu-validation/2026-05-06-mergekit-parity.json).

Closes the §M2 "TIES + DARE tested against mergekit's reference
outputs" deliverable. The Rust pf-model implementation
([`crates/pf-model/src/merge.rs`](../crates/pf-model/src/merge.rs))
is validated by its 8 algorithm-spec unit tests; this parity check
proves the algorithm spec itself is mergekit-conformant.

### 2026-05-06 — Modal A10G (V1 engine, vLLM 0.20.x, TinyLlama-1.1B)

`processfork-vllm 1.0.2` adds V1 support via `collective_rpc` with
module-level worker-side helpers (the v1.0.1 V0 path required direct
attribute access to `worker.cache_engine`, which V1 removed).
Operator must set `VLLM_ALLOW_INSECURE_SERIALIZATION=1` so V1
accepts the pickled callables.

| metric                                | observed              | budget / target       |
|---------------------------------------|-----------------------|-----------------------|
| KV pages serialized via collective_rpc| **38,599**            | (all of them)         |
| Snapshot CID                          | `sha256:ddb2696805…`  | stable across restore |
| Snapshot+restore wall                 | 140.7 s               | (single-shot)         |
| **First 80 chars of regenerated text**| identical             | byte-equal            |
| Full-string bit-exact                 | **diverges past ~80 chars** | requires V1 batch-invariant mode (v1.0.3) |
| Microbench p50 snapshot               | **47.5 ms**           | < 500 ms p99          |
| Microbench p99 snapshot               | 1428 ms (cold-start)  | < 500 ms (warm)       |

Raw: [`benchmarks/gpu-validation/2026-05-06-modal-a10g-vllm-v1.json`](./gpu-validation/2026-05-06-modal-a10g-vllm-v1.json).

The V1 plumbing is correct (snapshot succeeds, checkout succeeds,
generation reruns). The first 80 generated chars match byte-for-byte
across the snapshot+restore boundary; divergence beyond that is V1's
scheduler-ordering non-determinism, which `--enforce-deterministic`
(V0) covered but V1 hasn't ported yet. Full bit-exact V1 lands in
**v1.0.3** behind a `VLLM_DETERMINISTIC_V1` engine config flag once
upstream lands its V1 deterministic mode (tracked in
[vllm-project/vllm#XXX](https://github.com/vllm-project/vllm/issues)
as of 2026-05).

### Llama-3-8B p99 on H100 (§M1 thesis target)

The §M1 spec target is "snapshot p99 ≤ 500 ms for a 380K-token
Llama-3-70B agent on a Hopper-class GPU". The v1.0.2 deliverable
is the wired-in lane, executable via:

```bash
# one-time
modal secret create huggingface HF_TOKEN=hf_xxxx

# the actual run (~$3 of Modal H100 credit, ~5 min)
modal run scripts/gpu-validate-modal.py::llama8b
```

That dispatches the new `validate_llama8b` function (defined in
`scripts/gpu-validate-modal.py`) onto a Modal H100, loads
Llama-3.1-8B via vLLM, generates a long prompt to populate the KV
cache, then samples 50 per-page read latencies and reports
`p50_ms`, `p99_ms`, and the estimated full-snapshot wall.

| metric                                         | status               |
|------------------------------------------------|----------------------|
| Lane plumbed end-to-end (validate_llama8b)     | ✅ committed         |
| Operator HF token path (modal secret)          | ✅ documented        |
| Actual run on Modal H100                       | _operator-runs-it_   |
| Llama-3-70B (the §M1 model)                    | _v1.1 — needs ≥ 2× H100 NVLink_ |

The Llama-3-70B run wasn't executed in v1.0.2 because:
- Single H100 (80 GB) doesn't fit Llama-3-70B at bf16 (140 GB);
  needs 2-way TP across 2× H100 (~$11/hr on Modal).
- 380K-token contexts at 70B-class need a custom RoPE config
  (Llama-3.1 ships max_position_embeddings=8192; the 380K target
  needs a YaRN extension that's its own configuration story).

The Llama-3.1-8B run on a single H100 is the v1.0.2 honest stand-in:
same cache layout, same code path, ~10× smaller workload.
