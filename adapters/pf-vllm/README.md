# processfork-vllm

ProcessFork plugin for [vLLM](https://github.com/vllm-project/vllm) ≥0.10.
Adds OpenAI-compatible extended endpoints for snapshot / fork / checkout
that walk vLLM's paged KV cache via the batch-invariant kernel mode.

## Install

```bash
pip install "processfork-vllm[vllm]"
```

## Use

```bash
vllm serve meta-llama/Llama-3-8B \
  --enforce-deterministic \
  --plugin processfork
```

Then:

```
POST /v1/processfork/snapshot       { "name": "..." }
  → { "cid": "sha256:..." }

POST /v1/processfork/fork           { "cid": "...", "n": 12 }
  → { "cids": ["sha256:..."] }

POST /v1/processfork/checkout       { "cid": "..." }
  → { "ok": true }
```

Bit-exact restore requires `--enforce-deterministic` (stable since
vLLM 0.10). Without it, restore produces logits within ≤1e-4 of the
originals.

The wire format matches `agent_docs/cache-layer.md` —
`paged-batchinvariant-v1`. K and V pages are content-addressed
independently so a fork that mutates only V (one-token decode) shares
its K page with siblings.

## Status

The trait surface and the `paged-batchinvariant-v1` wire format are
stable. The live FFI shim into `vllm.worker.cache_engine` lands in
v1.0.1. Until then, the plugin's HTTP surface returns
`501 Not Implemented` with a clear pointer to this README.
