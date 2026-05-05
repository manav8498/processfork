# processfork-sglang

ProcessFork plugin for [SGLang](https://github.com/sgl-project/sglang) ≥0.5.
Snapshot / fork / checkout endpoints that preserve `RadixAttention`
prefix-sharing across restores.

## Install

```bash
pip install "processfork-sglang[sglang]"
```

## Use

```bash
python -m sglang.launch_server \
  --model meta-llama/Llama-3-8B \
  --plugin processfork \
  --deterministic-mode
```

Endpoints (matching the vLLM plugin's surface):

```
POST /processfork/snapshot
POST /processfork/fork
POST /processfork/checkout
POST /processfork/merge
```

The plugin maps SGLang's `mem_pool.req_to_token_pool` onto the
`paged-batchinvariant-v1` `LogicalSeq` array, so prefix-sharing
survives across snapshot/restore.

Bit-exact restore requires `--deterministic-mode`.

## Status

The trait surface and wire format are stable. The live FFI shim into
`sglang.srt.mem_cache.RadixCache` lands in v1.0.1 alongside the vLLM
adapter. Until then, the plugin's HTTP surface returns 501 with a
clear pointer to this README.
