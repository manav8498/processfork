# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

**v1.0.0 + v1.0.1 fully shipped.** All 12 build phases complete; 200+ tests
pass; live across 4 registries:

| Surface | What's live |
|---------|-------------|
| **PyPI `processfork`** | 1.0.1 × 5 platform wheels (macOS arm + macOS x86 + Linux x86_64 manylinux_2_28 + Linux aarch64 manylinux_2_28 + Windows x86_64) — published via OIDC Trusted Publishing |
| **PyPI 7 adapters**    | `processfork-{claude-code,langgraph,openinterpreter,vllm,sglang,autogen,crewai}` — all OIDC; vllm at 1.0.1, others at 1.0.0 |
| **crates.io**          | 8 crates @ 1.0.1: `processfork`, `pf-core`, `pf-model`, `pf-cache`, `pf-world`, `pf-effects`, `pf-merge`, `pf-registry` |
| **npm**                | `@processfork/sdk@1.0.2` (1.0.0/1.0.1 broken, fixed in 1.0.2) |
| **GHCR**               | `ghcr.io/manav8498/processfork:1.0.1` and `:latest` |
| **GitHub Releases**    | `v1.0.0`, `v1.0.1` — wheels + 3-arch `pf` binaries + cosign sigs |

Last green pipeline: https://github.com/manav8498/processfork/actions/runs/25411004140 (23/23).

## What's NOT done (intentional or deferred)

These are explicitly out of scope per CLAUDE.md `## Out of scope`:
- Hosted SaaS / web dashboard
- Native Windows runtime (we ship a Windows wheel of the SDK; the runtime
  itself is Linux/macOS only)
- Distributed multi-host fork
- Custom inference engine
- Custom model-merge algorithm
- Telemetry of any kind

## Open follow-ups (none blocking; all opt-in for the operator)

1. **Revoke leaked API tokens** that went through chat during v1.0.1 ship —
   PyPI (no longer needed at all thanks to OIDC), crates.io (replace with the
   90-day `processfork-release-ci` token already in repo secrets), npm (same).
   Operator action only.
2. **GPU-host bit-exact round-trip** against real Llama-3-8B vLLM — the
   wiring ships in `adapters/pf-vllm/processfork_vllm/plugin.py` and the
   test in `tests/test_vllm_smoke.py::test_live_vllm_bit_exact_replay` skips
   without `$PF_HAS_GPU=1`. Operator runs this on a CUDA box to flip the
   v1.0.1 status from "shipped (mock parity)" to "shipped (bit-exact)".
3. **Node.js 20 actions deprecation** (June 2026) — the workflow uses
   `actions/{checkout,setup-python,upload-artifact,download-artifact}@v4`
   plus `softprops/action-gh-release@v2`. All run on Node.js 20. Bump to
   the @v5 / @v6 lines whenever they land (pure mechanical follow).
4. **pyo3 0.24+** — current 0.22.6 has RUSTSEC-2025-0020 ignored in
   `deny.toml` because we don't call the affected fn. Bumping to 0.24
   needs the `Bound`-API #[pymodule] signature rewrite (~30 min). Not
   urgent; tracked in assumption A-007.

## How to do a v1.0.2 / vN.M.K release

The pipeline is now zero-touch given fresh tokens:

```bash
# 1. bump versions
#    Cargo.toml workspace.package.version
#    Cargo.toml workspace.dependencies pf-* version =
#    crates/pf-py/pyproject.toml [project] version
#    crates/pf-ts/package.json version
#    adapters/pf-<name>/pyproject.toml version (only if that adapter changed)
# 2. update CHANGELOG.md with a new ## [X.Y.Z] section
# 3. commit, then:
git tag vX.Y.Z && git push origin vX.Y.Z
```

The Release workflow (`.github/workflows/release.yml`) does the rest. PyPI
is OIDC; only `CARGO_REGISTRY_TOKEN` and `NPM_TOKEN` repo secrets are
needed, and both are valid 90 days from 2026-05-05.
