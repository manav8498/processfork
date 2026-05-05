# Cache layer

> Engineering source: [`agent_docs/cache-layer.md`](https://github.com/processfork/processfork/blob/main/agent_docs/cache-layer.md).

Captures vLLM's / SGLang's paged KV cache as a content-addressed
**page-manifest**, with K and V hashed independently so a fork
mutating only V (a one-token decode step) shares its K page with
siblings.

## Wire format — `paged-batchinvariant-v1`

```json
{
  "layout": "paged-batchinvariant-v1",
  "page_size_tokens": 16,
  "n_layers": 80,  "n_heads": 64,  "head_dim": 128,
  "dtype": "bf16",
  "pages": [
    { "ix": 0, "k": "sha256:…", "v": "sha256:…" },
    { "ix": 1, "k": "sha256:…", "v": "sha256:…" }
  ],
  "logical_seqs": [
    { "id": "seq-1", "page_ixs": [0, 1, 2], "fill_in_last_page": 7 }
  ]
}
```

## Bit-exact replay

Bit-exact restore requires the engine started in batch-invariant
kernel mode:

- vLLM: `--enforce-deterministic` (stable since v0.10).
- SGLang: `deterministic_mode=true`.

Throughput cost: 30–60 % depending on workload. Default ProcessFork
mode is **near-exact** (≤1e-4 logit deviation tolerated); `--exact`
opts into batch-invariant.

## API (Rust)

```rust
use pf_cache::{capture_cache, restore_cache, CachePager, SyntheticCachePager};

let cid = capture_cache(&mut pager, &blobs)?;
restore_cache(&mut destination_pager, &blobs, &cid)?;
```

Adapters implement [`CachePager`] for the engine of their choice; v1
ships [`SyntheticCachePager`] for tests and the
[vLLM](../integrations/vllm.md) / [SGLang](../integrations/sglang.md)
plugins for the live integrations.
