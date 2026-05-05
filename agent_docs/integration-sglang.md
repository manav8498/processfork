# Integration: SGLang

A native SGLang plugin equivalent to the vLLM one. Endpoints:

```
POST /processfork/snapshot
POST /processfork/fork
POST /processfork/checkout
POST /processfork/merge
```

Install:

```bash
python -m sglang.launch_server \
  --model meta-llama/Llama-3-8B \
  --plugin processfork \
  --deterministic-mode
```

SGLang's `RadixAttention` plays well with our content-addressed paging — the
radix prefix-tree maps cleanly to the page-manifest's `logical_seqs[]`. We
preserve prefix-sharing across fork boundaries for free.

Bit-exact restore requires `--deterministic-mode`.

Same example coverage as vLLM under `examples/03-cross-machine/`.
