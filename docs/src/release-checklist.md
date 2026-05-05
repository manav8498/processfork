# v1.0 release checklist

> Source: [`agent_docs/release-checklist.md`](https://github.com/processfork/processfork/blob/main/agent_docs/release-checklist.md).

## Pre-flight

- [x] `cargo test --workspace` — green (154 tests on macOS arm64).
- [x] `cargo clippy --workspace -D warnings` — clean.
- [x] `cargo fmt --all -- --check` — clean.
- [x] `pytest adapters/` — 36 passed, 2 GPU-gated skips.
- [x] Microbench in budget (snapshot 7.9 ms vs 500 ms).
- [x] PFBench self-test green (echo model).
- [x] `pf --help` lists 12 (+ completions) subcommands.
- [x] mdBook builds locally.
- [x] All 8 examples present; 5 runnable on the build host.
- [x] `examples/02-cli-snapshot/run.sh` exits 0.
- [ ] **Operator-supplied:** real GPU bit-exact replay (`$PF_HAS_GPU=1`).
- [ ] **Operator-supplied:** SWE-Bench Verified ≥ 15 pp uplift.

## Tag

- [x] CHANGELOG `[Unreleased]` reads as `v1.0.0`.
- [x] Bumping `[workspace.package].version → 1.0.0` (deferred to the
      release commit; the dev workspace tracks `0.1.0-dev`).
- [ ] `git tag -s v1.0.0 -m "ProcessFork v1.0.0"`.

## Publish (operator-supplied tokens)

- [ ] GitHub Actions release workflow triggers on tag.
- [ ] `cargo publish` for each of the 8 publishable crates.
- [ ] `maturin publish` for `processfork` on PyPI.
- [ ] `npm publish` for `@processfork/sdk`.
- [ ] `docker buildx push ghcr.io/<owner>/processfork:1.0.0`.
- [ ] All 7 adapter pkgs pushed to PyPI as `processfork-<adapter>`.

## Post

- [ ] `cargo install processfork` from a fresh shell works.
- [ ] `pip install processfork` from a fresh venv works.
- [ ] `npm install @processfork/sdk` from a fresh node project works.
- [ ] Docs site deployed (GitHub Pages or Mintlify).
- [ ] Demo recording on the README.
- [ ] Announce.
