# Cache layer

The hardest layer. KV-cache is the fast-path memory of the agent — losing it
means re-prefilling 380K tokens (~minutes on H100). Capturing it bit-exact
requires both a stable serialization format and batch-invariant inference
kernels.

## On-disk format: `paged-batchinvariant-v1`

A KV-cache is a flat tensor split into fixed-size pages (vLLM default 16
tokens/page). We content-address each page individually:

```
page_manifest.json
{
  "layout": "paged-batchinvariant-v1",
  "page_size_tokens": 16,
  "n_layers": 80,
  "n_heads": 64,
  "head_dim": 128,
  "dtype": "bf16",
  "pages": [
    { "ix": 0,    "k": "sha256:…", "v": "sha256:…" },
    { "ix": 1,    "k": "sha256:…", "v": "sha256:…" },
    …
  ],
  "logical_seqs": [
    { "id": "seq-1", "page_ixs": [0, 1, 2, …], "fill_in_last_page": 7 }
  ]
}
```

- Each `page` blob is `n_layers * page_size_tokens * n_heads * head_dim * 2 bytes`
  (bf16) per K and V, then zstd-19 compressed.
- `logical_seqs` map per-request token-ID arrays back to physical pages.
- CoW across forks is **automatic** because identical pages share their digest.

## Adapters

### vLLM ≥0.10

vLLM exposes the paged KV cache via the `PagedAttentionMetadata` struct on the
worker. We add a sidecar process that:

1. Sends `pause` to the worker (drains the in-flight batch).
2. Reads the page table out of `worker.cache_engine.gpu_cache`.
3. For each page, computes its SHA-256 in the GPU and DMA-streams compressed
   pages to disk via `nvidia-smi --gpu-reset`-safe pinned-memory ringbuffer.
4. Writes `page_manifest.json` referencing all page digests.
5. Sends `resume` to the worker.

Bit-exact mode requires the worker started with `--enforce-deterministic`
(stable since 0.10). Restore reverses: page manifest → DMA into freshly-
allocated `gpu_cache` pages → patch worker page table → resume.

### SGLang ≥0.5

Similar mechanism via SGLang's `RadixAttention` + `mem_pool.req_to_token_pool`.
The adapter lives in `adapters/pf-sglang/`.

## Batch-invariant kernel notes

Default attention kernels are NOT batch-invariant: the kernel's reduction
order depends on batch shape, producing tiny (1e-6 to 1e-4) numerical
differences. Bit-exact replay requires:

- vLLM: `--enforce-deterministic` (uses fixed-shape kernels).
- SGLang: `deterministic_mode=true` in the engine config.
- Throughput cost: 30–60 % depending on workload.

Default ProcessFork mode is **near-exact** (≤1e-4 logit deviation tolerated).
`--exact` opts into batch-invariant for compliance / debug use cases.

## Build-host caveats

The agent build host is macOS arm64. vLLM and SGLang require CUDA. The cache-
layer code path is implemented and unit-tested with synthetic page fixtures.
The real round-trip integration test (`tests/cache_round_trip_vllm.rs`) is
gated behind `$PF_HAS_GPU=1` and skipped on non-CUDA hosts.

**v1.0.1 GPU validation (2026-05-06)**: ran the full bit-exact round-trip
on Modal A10G against vLLM 0.6.6 + TinyLlama-1.1B. Result: 38 619 KV pages
snapshotted, restored, regenerated text byte-identical
(`out_a == out_b`). Snapshot p50 42 ms (well under the 500 ms p99 budget).
Raw JSON in `benchmarks/gpu-validation/2026-05-06-modal-a10g.json`.

vLLM ≥0.10 ships V1 (subprocess-worker architecture) which needs the
v1.0.2 `engine_core.collective_rpc('get_cache_engine')` rewrite of the
pager — V0's directly-attribute-accessible `CacheEngine` is what the
v1.0.1 adapter targets.

## Failure modes

- **GPU OOM during capture**: detected via `cudaMemGetInfo` pre-flight; abort
  with clear error. Never partial-write the page manifest.
- **Worker crash mid-capture**: pages already written are GC-able by digest;
  page manifest never written → image never assembled.
- **Page count mismatch on restore**: refuse to load. Manifest invalid.
