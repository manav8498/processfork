# ProcessFork build plan (live)

> **First three actions every new session:**
> 1. `cat claude-progress.json` (machine state)
> 2. `cat claude-plan.md`        (this file)
> 3. `git status && git log --oneline -20`
> Then read `agent_docs/<phase-name>.md` for the phase you're picking up.

## Where I am right now

**9 of 12 phases complete and tagged. 143 tests pass (133 Rust + 5 Python +
5 TypeScript). Lints clean. Workspace is at HEAD = `phase-8-complete`.**

| Phase | Name              | Status | Tag                | Tests   |
|-------|-------------------|--------|--------------------|---------|
| 0     | bootstrap         | ✅ done | phase-0-complete  | —       |
| 1     | core_engine_rust  | ✅ done | phase-1-complete  | 16      |
| 2     | world_layer       | ✅ done | phase-2-complete  | 12      |
| 3     | effects_layer     | ✅ done | phase-3-complete  | 18      |
| 4     | cache_layer       | ✅ done | phase-4-complete  | 21      |
| 5     | model_layer       | ✅ done | phase-5-complete  | 24      |
| 6     | merge_engine      | ✅ done | phase-6-complete  | 31      |
| 7     | sdks              | ✅ done | phase-7-complete  | 5+5     |
| 8     | cli               | ✅ done | phase-8-complete  | 11      |
| 9     | registry          | ▶ next | —                  | —       |
| 10–12 | …                 | ⏳ pend | —                  | —       |

`pf` CLI is now wired end-to-end:
- 10 wired subcommands (snapshot/fork/checkout/merge/log/diff/status/gc/verify/completions)
- 3 stub subcommands (push/pull/clone) returning exit 2 with a Phase-9 pointer
- Exit codes 0/1/2/3/4 per `agent_docs/cli-spec.md` verified by integ tests
- `examples/02-cli-snapshot/run.sh` runs end-to-end against the binary

## What's next (top of stack — Phase 9: registry)

Phase 9 is **registry adapters**. Spec lives in
`agent_docs/registry-spec.md`. Four backends behind one trait:

```rust
#[async_trait]
pub trait Registry: Send + Sync {
    async fn push(&self, manifest: &Manifest, blobs: &dyn BlobStore) -> Result<()>;
    async fn pull(&self, image_ref: &ImageRef) -> Result<(Manifest, Vec<(Digest256, Vec<u8>)>)>;
    async fn exists(&self, image_ref: &ImageRef) -> Result<bool>;
}
```

1. **`pf-registry::ImageRef`**: parser for `hf://user/repo[:tag]`,
   `s3://bucket/prefix`, `ipfs://CID`, `oci://host:port/repo[:tag]`,
   `file://path` (the local-OCI testbed).
2. **`pf-registry::FileRegistry`**: dead-simple file-system-backed
   registry — copies `manifest.json` + every reachable blob into a
   target directory. The build-host integration test backbone; runs
   without external services.
3. **`pf-registry::HfRegistry`**: stores manifest + blobs against the
   Hugging Face Hub via `reqwest`. Auth via `HF_TOKEN`. **Live test
   gated by `$HF_TOKEN`**.
4. **`pf-registry::S3Registry`**: S3 / R2 / MinIO. **Live test gated
   by `$AWS_ACCESS_KEY_ID`**.
5. **`pf-registry::IpfsRegistry`** (feature-flag `ipfs`): pin manifest
   + blobs against a local IPFS daemon (`http://127.0.0.1:5001`).
6. **`pf-registry::sign`**: cosign-style signing of the canonical-JSON
   manifest bytes. v1 ships keyless (Sigstore Fulcio) by default;
   key-file via `--key`. For Phase 9 we ship the wire format + verify
   path; the full Fulcio dance is feature-gated.
7. **CLI wiring**: replace the `commands/stub.rs` `push`/`pull`/`clone`
   bodies with real calls into `pf-registry`.

For Phase 9 I'll ship the trait + `FileRegistry` fully end-to-end
(testable on the build host without any external services). HF + S3 +
IPFS get scaffolded with the trait surface + URL parsing + auth-token-
aware integration tests gated behind their respective env vars.

## Blockers

- **None for Phase 9** as scoped above. Live HF / S3 / IPFS round-trips
  need operator creds and live in CI gated jobs.

## Recently completed (this session)

- Phase 8 (CLI): refactored main.rs into commands/ tree; wired 10
  subcommands to layer crates; stubbed 3 to Phase 9 with exit 2; added
  global `--store`/`--no-color`/`-v...`; 11 assert_cmd integration
  tests; `examples/02-cli-snapshot/run.sh` runnable demo.

## Files most likely to need editing in the next session

- `crates/pf-registry/Cargo.toml` — drop the unused `reqwest` for the
  trait + `FileRegistry`; add `async-trait`. Add features
  `ipfs`, `s3`, `hf` for the gated backends.
- `crates/pf-registry/src/lib.rs` — re-architect from Phase-0 stub.
- `crates/pf-registry/src/{image_ref,registry,file,hf,s3,ipfs,sign}.rs`
  (new).
- `crates/pf-registry/tests/registry_round_trip.rs` (new) —
  FileRegistry round-trip is the build-host test; HF / S3 are gated.
- `crates/pf-cli/src/commands/{push,pull,clone}.rs` (new) — replace
  the stub bodies with thin wrappers around `pf-registry`.
- `claude-progress.json` — flip phase 9 to done when gate passes.

## Operator-only deliverables (cannot run from build agent)

These remain blocked on operator action, not on code:
- `pip install processfork` end-to-end smoke from PyPI (needs
  `PYPI_API_TOKEN`). The wheel itself builds and installs locally.
- `npm install @processfork/sdk` end-to-end smoke from npm (needs
  `NPM_TOKEN`). The .node binary builds and runs locally.
- `cargo install processfork` from crates.io (needs `CARGO_REGISTRY_TOKEN`).
- 60-second asciinema demo recording (script lives under `demo/` once it's
  written in Phase 12; recording is operator-produced).
- Real-hardware bit-exact replay test (needs CUDA host + Llama-3-8B served by
  vLLM ≥0.10 in deterministic mode; gated behind `$PF_HAS_GPU=1`).
- mergekit-equivalence test (needs Llama-3-8B base weights + Python
  `mergekit` install; gated behind `$PF_HAS_GPU=1`).
- Live summarizer call for trace-merge (needs Anthropic API key; gated
  behind the `live-summarizer` feature flag).
- Live HF Hub push/pull (needs `HF_TOKEN`; gated test).
- Live S3 push/pull (needs AWS creds; gated test).

## Context-window discipline reminders

- 60 % → write a one-paragraph progress note here.
- 70 % → commit WIP behind a feature flag if needed; consider compact.
- 85 % → finish the current logical unit; stop adding new work; leave clean
  state files for the next session.
