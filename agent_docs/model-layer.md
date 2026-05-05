# Model layer

Captures the difference between the running model's weights and a known base.
Three classes:

| Kind          | Storage shape                                                    |
|---------------|------------------------------------------------------------------|
| `Lora`        | `[layer_id][matrix][rank, in/out]` low-rank adapters             |
| `IA3`         | `[layer_id][per-head scaling vector]`                            |
| `Full`        | `[param_name][dense delta]` (rare; very large)                   |
| `InPlaceTtt`  | `[step_id][param_name][dense delta]` (test-time training trace)  |

All are stored as zstd-compressed safetensors-style files referenced from the
`model.diff` digest in the manifest. The `model.base` digest points at the
base-model fingerprint; if the base is on Hugging Face we store the HF hash
rather than the full ~140 GB blob.

## TIES + DARE merge

For `pf merge B -> A` where both have weight diffs Δ_A and Δ_B from common
ancestor X:

- **DARE**: drop random magnitudes from Δ_A and Δ_B with probability `p`
  (default 0.7), rescale survivors by `1/(1-p)`. Reduces interference.
- **TIES**: trim, elect sign (majority vote), disjoint-merge surviving values.

Reference implementation lives at https://github.com/arcee-ai/mergekit. Our
merge unit-tests assert byte-equivalent output to mergekit on a fixed seed.

## API surface (Rust)

```rust
pub trait ModelDiff {
    fn kind(&self) -> DiffKind;
    fn capture(handle: &dyn ModelHandle) -> Result<Self> where Self: Sized;
    fn apply(&self, base: &dyn ModelHandle) -> Result<()>;
    fn ties_dare_merge(&self, other: &Self, alpha: f32) -> Result<Self> where Self: Sized;
}
```

`ModelHandle` is the runtime-specific pointer to the live model (vLLM
`LLMEngine`, SGLang engine, raw safetensors-on-disk, etc.).

## Build-host caveat

We have no GPU. Merge unit tests use 8 × 8 toy weight matrices and assert
TIES+DARE invariants symbolically. The full mergekit-equivalence test is
gated `$PF_HAS_GPU=1` because it needs a real Llama-3-8B base.
