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
stable. The mock-mode K/V page round-trip is regression-tested
locally (no GPU needed). Live GPU validation runs on Modal — see
`scripts/gpu-validate-modal.py` and the JSONs in
`benchmarks/gpu-validation/`.

## Bit-exact replay: V0 vs V1 engine

`benchmarks/gpu-validation/2026-05-06-modal-a10g.json` shows
**`bit_exact: true`** on the **V0 engine** (TinyLlama-1.1B, 38 619
KV pages, regenerated text byte-identical across snapshot/restore).
`benchmarks/gpu-validation/2026-05-06-modal-a10g-vllm-v1.json` shows
**`bit_exact: false`** on the **V1 engine** with output-equivalent
first-80-chars match — V1's `collective_rpc` worker scheduling has
internal non-determinism (kernel launch ordering, KV-cache slot
allocation) that ProcessFork cannot eliminate from the outside.

### Workaround: pin to V0 + `enforce_eager=True` (v1.0.12)

If you need byte-identical regenerated output across
snapshot/restore today:

```bash
# Server mode:
vllm serve meta-llama/Llama-3-8B \
    --enforce-eager \
    --enable-prefix-caching \
    # do NOT pass --use-v2-block-manager / --enforce-deterministic alone;
    # those run the V1 engine path on recent vllm. Pin V0 explicitly:
    --engine-version v0
```

```python
from vllm import LLM, SamplingParams
llm = LLM(
    model="meta-llama/Llama-3-8B",
    enforce_eager=True,           # disables CUDA graph reuse
    # On vllm ≥0.10 the V1 engine is the default; pass explicitly:
    engine_args={"engine_version": "v0"},
)
```

Caveats:

- `enforce_eager=True` disables CUDA graphs, so throughput drops
  (typically 1.3–1.8× slower than V1 + graphs on Hopper-class GPUs).
  Pay this when bit-exactness matters; skip it when output-equivalent
  is enough.
- V0 is feature-frozen upstream as of vllm 0.10. New scheduling
  features land on V1 only. Plan to migrate to V1 + accept output-
  equivalent once upstream lands deterministic batch scheduling
  (tracked in vllm/issues — search "deterministic V1").
- The V1 output-equivalent path is still useful: the first ~80
  generated chars match across snapshot/restore on the Modal lane,
  so for resume-and-continue agent workflows (vs. RL rollouts that
  need byte-identical traces) V1 + `collective_rpc` is fine.

If you're snapshotting an agent that does its own continuation
(`continue_from_kv_cache=True` semantics), the resumed branch on V1
will diverge after ~80 chars but stay coherent — i.e. it will not
lose the prefix, only sample stochastically from a slightly
different distribution. Use V0 for RL-rollout reproducibility; V1
is fine for "snapshot before destructive change, resume after."

## Status of v1.0.x runtime

The local PF_HAS_GPU=1 paths in the repo's `examples/06` are
*skeletons* — they exit 2 with a Modal-lane pointer. The validation
IS the Modal lane. For interactive use of the adapter on your own
CUDA box, install `processfork-vllm[vllm]` and call the SDK from
inside your engine process. The mock-mode tests in `tests/` cover
the page persistence + restore code path on every CI host.
