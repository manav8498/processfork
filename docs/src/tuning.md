# Performance tuning

ProcessFork's hot paths are CAS hashing (SHA-256) and zstd-19
compression on push. Dial the trade-offs:

## Snapshot speed vs storage

| Setting                       | Faster | Smaller | Default              |
|-------------------------------|:------:|:-------:|----------------------|
| `--exact` (batch-invariant)   |        |         | off (~30–60% faster) |
| zstd level                    |   ↓    |   ↑     | 19                   |
| `PF_CACHE_BUDGET_MB`          |   ↑    |   ↓     | 4096                 |

For latency-sensitive snapshots, drop zstd to level 3:

```bash
PF_ZSTD_LEVEL=3 pf snapshot ...
```

(planned environment knob; currently zstd-19 is hard-coded — the
v1.1 work item is to make this a CLI flag.)

## Storage efficiency

CAS dedup is automatic. Two pushes of the same image into the same
registry copy zero new bytes. Two snapshots of a near-identical
agent state grow the store by ~1× the number of differing pages
(K + V are addressed independently).

The 12-fork ÷ 1-fork ratio holds well below 1.5× on the synthetic
fixture; in practice on real agent workloads the gap is wider since
most pages are shared.

## Concurrency

The snapshot orchestrator fans out to OS threads (one per layer) via
`thread::scope`. Capture is CPU-bound by zstd; throughput scales
linearly up to the hash-and-compress core count.

## On `--exact`

`--exact` enables vLLM `--enforce-deterministic` / SGLang
`deterministic_mode`. Throughput drops 30–60 % depending on workload;
default mode tolerates ≤1e-4 logit deviation across machines of the
same architecture.

Use `--exact` when:
- You need cross-machine bit-equal restore (compliance, science).
- You're debugging a non-determinism that shows up at the model
  layer.

Default to off otherwise.
