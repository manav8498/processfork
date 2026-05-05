# Contributing

> Source: [`CONTRIBUTING.md`](https://github.com/manav8498/processfork/blob/main/CONTRIBUTING.md).

## Pull request requirements

- `cargo fmt --all -- --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace` passes.
- `pytest adapters/` passes (all the dep-gated tests should still
  pytest-skip cleanly without their optional extras).
- Coverage delta is non-negative (`cargo llvm-cov --summary-only`).
- Every new public API has a doc-comment with a runnable example.
- New dependencies are justified in the commit body.

## Commit format

Conventional Commits. End with `Co-Authored-By:` if your editor /
agent did the work.

## Code of conduct

Contributor Covenant v2.1. Report incidents to
**conduct@processfork.dev**.
