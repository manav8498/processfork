# Performance budget

| Operation                                  | p99 budget |
|--------------------------------------------|------------|
| `pf snapshot` of 380K-token agent on H100  | ≤ 500 ms   |
| `pf fork -n 12` (CoW metadata only)        | ≤ 100 ms   |
| `pf checkout` of 1.2 GB image, cold cache  | ≤   5 s    |
| Storage 12-fork ÷ storage 1-fork           | ≤   1.5×   |

## Build-host measurements (no GPU)

Captured from `cargo bench --workspace` on macOS arm64 with the
synthetic 4-layer fixture (32 cache pages × 16 KiB + 64 fs files ×
4 KiB + 64 KiB model diff ≈ 1.4 MB total payload):

| metric                           | observed   |
|----------------------------------|------------|
| `snapshot_synthetic_4layer`      | **7.9 ms** |
| `cache_capture_64_pages`         | **531 µs** |
| `cache_restore_64_pages`         | **34 µs**  |
| Identical-content second snapshot| **614 B** growth |

## GPU-host measurements (Modal A10G, 2026-05-06)

Captured from `modal run scripts/gpu-validate-modal.py` against
TinyLlama-1.1B on a 24 GB A10G (vLLM 0.6.6, V0 engine):

| metric                                     | observed         | budget        |
|--------------------------------------------|------------------|---------------|
| Snapshot p50 (warm, 64 × 4 KiB fixture)    | **42 ms**        | < 500 ms p99  |
| Snapshot min (steady-state)                | **41 ms**        | —             |
| Snapshot p99 (incl. cold-start)            | 1180 ms          | warm only     |
| **Bit-exact KV-cache replay**              | **✅ verified**  | `out_a == out_b` |
| KV pages snapshotted + restored            | 38 619           | (all)         |
| TIES + DARE real-shape Frobenius Δ         | 0.0              | identical     |

Raw JSON: [`benchmarks/gpu-validation/`](https://github.com/manav8498/processfork/tree/main/benchmarks/gpu-validation).
vLLM ≥0.10 (V1 engine, subprocess-worker architecture) needs the
v1.0.2 `engine_core.collective_rpc('get_cache_engine')` rewrite.

Larger Llama-3-8B p99 numbers on H100/A100 are operator-runs-it on a
beefier GPU lane.

## Tuning knobs

- `PF_CACHE_BUDGET_MB` — hot-page LRU budget (default 4096).
- `--exact` flag on `pf snapshot` — opt into batch-invariant kernels
  for bit-exact replay (~30–60 % throughput cost).
- `--scrub-env <regex>` — redact env vars before sealing.

See [Performance tuning](./tuning.md) for production-deployment
guidance.
