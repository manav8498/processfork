# Contributing to ProcessFork

We accept contributions, but ProcessFork is opinionated. Read this first.

## Before you file

1. Check `agent_docs/feature-spec.md` to see whether your idea is in scope
   for v1 or deferred to v2.
2. Check `claude-progress.json/deferred_to_v2[]` and the GitHub issues
   labelled `v2` — your idea may already be tracked.

## Pull request requirements

- `cargo fmt --all -- --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace` passes.
- Coverage delta is non-negative (`cargo llvm-cov --summary-only`).
- For new public APIs: doc-comment with a runnable example.
- For new dependencies: justification in the commit body and an entry in
  `cargo deny.toml` if it introduces a non-MIT/Apache/BSD license.

## Commit format

Conventional Commits. Examples:
- `feat(cache): paged KV serialization for vLLM ≥0.10`
- `fix(world): respect .pfignore on macOS`
- `docs(merge): clarify --replay-effects semantics`

## Sign-off

We use Developer Certificate of Origin (DCO). Add `Signed-off-by: Name
<email>` to every commit (`git commit -s`).

## Code of conduct

By participating you agree to abide by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
