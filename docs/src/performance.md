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

Real bit-exact replay against vLLM ≥0.10 + Llama-3-8B is
operator-runs-it (`$PF_HAS_GPU=1`). Numbers land in
[`benchmarks/RESULTS.md`](https://github.com/processfork/processfork/blob/main/benchmarks/RESULTS.md)
when the operator runs the gated lane.

## Tuning knobs

- `PF_CACHE_BUDGET_MB` — hot-page LRU budget (default 4096).
- `--exact` flag on `pf snapshot` — opt into batch-invariant kernels
  for bit-exact replay (~30–60 % throughput cost).
- `--scrub-env <regex>` — redact env vars before sealing.

See [Performance tuning](./tuning.md) for production-deployment
guidance.
