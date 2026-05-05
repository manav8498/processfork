# Rust crate

Install:

```bash
cargo add processfork              # convenience meta-crate (planned)
# Or per-crate:
cargo add pf-core pf-merge pf-registry
```

## Workspace

The Rust workspace ships eight crates plus two binding crates and
two registry/CLI crates:

| Crate         | Role                                                  |
|---------------|-------------------------------------------------------|
| `pf-core`     | CAS, manifest, atomic snapshot orchestrator           |
| `pf-model`    | Weight diffs + TIES + DARE                            |
| `pf-cache`    | Paged KV-cache wire format + adapter trait            |
| `pf-world`    | FS / env / processes / DOM capture                    |
| `pf-effects`  | HMAC-chained tool-call ledger + replay policy         |
| `pf-merge`    | Three-way merge engine                                |
| `pf-registry` | File / HF / S3 / IPFS / OCI adapters                  |
| `processfork`    | The `pf` CLI binary                                       |
| `pf-py`       | pyo3 bindings (powers `processfork` on PyPI)          |
| `pf-ts`       | napi-rs bindings (powers `@processfork/sdk` on npm)   |

## Quick reference

```rust
use std::sync::Arc;
use pf_core::store::PfStore;
use pf_core::manifest::AgentInfo;
use pf_core::snapshot::Snapshotter;
use pf_core::fixture::*;

let store = PfStore::open("~/.processfork")?;
let agent = AgentInfo {
    kind: "demo".into(), version: "0".into(), fingerprint: "h".into(),
};
let snapper = Snapshotter::new(
    agent,
    Arc::new(FixtureModelCapture(FixtureSpec::default())),
    Arc::new(FixtureCacheCapture(FixtureSpec::default())),
    Arc::new(FixtureWorldCapture(FixtureSpec::default())),
    Arc::new(FixtureEffectsCapture(FixtureSpec::default())),
    Arc::new(FixtureTraceCapture(FixtureSpec::default())),
);
let cid = snapper.snapshot(&store, vec![])?;
```

Auto-generated docs land at <https://docs.rs/pf-core> per crate when
v1.0 publishes to crates.io.
