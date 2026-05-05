# Model layer

> Engineering source: [`agent_docs/model-layer.md`](https://github.com/processfork/processfork/blob/main/agent_docs/model-layer.md).

Captures the difference between the running model's weights and a
known base. Four kinds:

| Kind          | Storage shape                                          |
|---------------|--------------------------------------------------------|
| `Lora`        | `[layer_id][matrix][rank, in/out]` low-rank adapters   |
| `IA3`         | `[layer_id][per-head scaling vector]`                  |
| `Full`        | `[param_name][dense delta]` (rare; very large)         |
| `InPlaceTtt`  | `[step_id][param_name][dense delta]` TTT trace         |

All persist as zstd-compressed JSON envelopes (`model.diff.v1`)
referenced from the manifest's `model.diff` digest. The
`model.base` digest points at the base-model fingerprint; if the base
lives on Hugging Face we store the HF hash, not the ~140 GB blob.

## TIES + DARE merge

`pf merge` reduces both sides' weight diffs Δ_A and Δ_B from common
ancestor X via task arithmetic:

- **DARE**: drop random magnitudes with probability `p` (default
  `0.7`), rescale survivors by `1/(1-p)`. Reduces interference.
- **TIES**: trim, sign-elect (majority vote by magnitude), disjoint-
  merge surviving values, scale by `α` (default `0.5`).

Reference: [TIES](https://arxiv.org/abs/2306.01708),
[DARE](https://arxiv.org/abs/2311.03099),
[`mergekit`](https://github.com/arcee-ai/mergekit).

The build-host tests use small synthetic tensors; the
mergekit-equivalence test on Llama-3-8B base weights is operator-
runs-it (`$PF_HAS_GPU=1`).

## API (Rust)

```rust
use pf_model::{ModelDiff, store_diff, load_diff, ties_merge, dare, TiesParams};

let cid     = store_diff(&blobs, my_diff)?;       // persist
let merged  = ties_merge(&[&va, &vb], TiesParams::default())?;
let dropped = dare(&delta, 0.7, seed)?;
```
