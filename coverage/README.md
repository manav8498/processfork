# Coverage

Per §M8 of the v1.0 megaprompt, ProcessFork ships with continuous
line-coverage measurement and a CI ratchet.

## Latest baseline (2026-05-06)

| metric    | observed   | target |
|-----------|------------|--------|
| Lines     | **88.96%** | ≥ 85%  |
| Regions   | **88.37%** | ≥ 85%  |
| Functions | **78.31%** | (info) |

Measured over the entire Rust workspace via `cargo-llvm-cov 0.8.5`,
**excluding** `crates/pf-py` and `crates/pf-ts` — those are cdylib
language-binding shims that are exercised end-to-end from the Python
and TypeScript SDK test suites, not from Rust unit tests, so their
0% Rust-side coverage is by design.

Raw machine-readable JSON: [`baseline.json`](./baseline.json).

## Reproduce locally

```bash
cargo install cargo-llvm-cov   # one-time
cargo llvm-cov --workspace \
    --no-default-features \
    --features="hf-live oci-live s3-live ipfs-live" \
    --ignore-filename-regex "pf-py|pf-ts" \
    --summary-only
```

## CI gate

`.github/workflows/ci.yml` runs the same command on every PR and
fails the build if line coverage drops below **85%** (the spec's
floor). The check is in the `coverage` job; see the workflow for the
exact threshold and ignore-regex.
