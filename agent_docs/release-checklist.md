# v1.0.0 release checklist

Mirror of `feature-spec.md` reduced to a literal sequence the release runner
follows. Every item must be ✅ and verifiable from a fresh clone.

## Pre-flight

- [ ] `cargo test --workspace` — green on Ubuntu 24.04 (x86_64 + arm64) and
      macOS 14 (arm64).
- [ ] `cargo llvm-cov --workspace --summary-only` — line coverage ≥85%.
- [ ] `cargo bench --workspace` — every microbench within budget
      (see agent_docs/benchmarks.md).
- [ ] `cargo deny check` — zero advisories.
- [ ] `cargo audit` — zero unfixed CVEs.
- [ ] `pytest -x adapters/*/python-tests/` — green.
- [ ] `npm test` in `crates/pf-ts/` — green.
- [ ] Bit-exact replay test (gated `$PF_HAS_GPU=1`) — green on operator GPU
      box.
- [ ] All 7 integration adapters run their `examples/<name>/` end-to-end.
- [ ] All 8 examples under `examples/` exit 0.
- [ ] `pf --help` lists 12 subcommands; each subcommand `--help` is complete.
- [ ] mdBook builds: `mdbook build docs/`.
- [ ] Landing page builds and previews locally.
- [ ] 60-second demo asciinema script runs end-to-end (operator records).

## Tag

- [ ] Bump `[workspace.package].version` to `1.0.0` in `Cargo.toml`.
- [ ] Bump SDK versions in `pyproject.toml` and `package.json`.
- [ ] Update `CHANGELOG.md` with the v1.0.0 stanza.
- [ ] Commit: `chore(release): v1.0.0`.
- [ ] Tag: `git tag -s v1.0.0 -m "ProcessFork v1.0.0"`.
- [ ] Push: `git push origin v1.0.0`.

## CI publish (operator-supplied tokens required — see assumption A-003)

- [ ] GitHub Actions release workflow triggers on tag.
- [ ] Builds signed binaries for ubuntu-24.04 (x86_64 + arm64) and
      macos-14 (arm64).
- [ ] Uploads binaries + SBOMs to GitHub Releases (cosign-signed).
- [ ] `cargo publish -p pf-core … pf-cli` (8 crates in order) →
      crates.io.
- [ ] `maturin publish` from `crates/pf-py/` → PyPI.
- [ ] `npm publish` from `crates/pf-ts/` → npm.
- [ ] `docker buildx push ghcr.io/<owner>/processfork:1.0.0`.

## Post

- [ ] `cargo install processfork` from a fresh shell works.
- [ ] `pip install processfork` from a fresh venv works.
- [ ] `npm install @processfork/sdk` from a fresh node project works.
- [ ] Landing page deployed to GitHub Pages.
- [ ] mdBook deployed to docs.processfork.dev (or GitHub Pages subpath).
- [ ] Demo video uploaded to README.
- [ ] Announce on X / HN / r/MachineLearning (operator-driven).
