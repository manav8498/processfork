# Integration: vLLM

A native vLLM server plugin that adds OpenAI-compatible extended endpoints:

```
POST /v1/processfork/snapshot       { "name": "..." }                -> { "cid": "sha256:..." }
POST /v1/processfork/fork           { "cid": "...", "n": 12 }       -> { "cids": ["sha256:..."] }
POST /v1/processfork/checkout       { "cid": "..." }                 -> { "ok": true }
POST /v1/processfork/merge          { "from": "...", "into": "..." } -> { "outcome": "clean" }
```

Install:

```bash
vllm serve meta-llama/Llama-3-8B \
  --plugin processfork \
  --enforce-deterministic
```

The plugin:
- Pauses the worker, walks the paged KV cache, hashes pages on-GPU, DMA-
  streams to disk via pinned-memory ringbuffer (see cache-layer.md).
- Captures the model layer as a (base, diff) pair where base = HF model hash.
- World layer is empty (vLLM is stateless WRT FS).
- Effects layer is populated by client-side `ToolProxy` (the inference server
  doesn't see tool calls; client does).

Bit-exact restore requires `--enforce-deterministic`.

`examples/03-cross-machine/` snapshots a vLLM session on machine A, pushes to
HF Hub, restores on machine B. Asserts logit-identical next-token output.
