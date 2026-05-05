---
name: napi-binding
description: Patterns for exposing ProcessFork Rust APIs to TypeScript via napi-rs 2.x.
---

# napi-rs binding patterns

## Function exposure

```rust
use napi_derive::napi;

#[napi]
pub async fn snapshot(agent_id: String) -> napi::Result<String> {
    pf_core::snapshot(&agent_id)
        .await
        .map(|cid| cid.to_string())
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}
```

`async fn` exposed via `#[napi]` returns a JS `Promise` automatically when the
`tokio_rt` feature is enabled (already on in workspace).

## Build

`napi build --release` from `crates/pf-ts/`. Outputs an `index.node` per
target. Cross-compile in CI: `napi build --target aarch64-apple-darwin`,
etc.

## Package

`crates/pf-ts/package.json` declares the `napi.binaryName` and per-platform
optional deps. Users `npm install @processfork/sdk` and napi resolves the
right binary automatically.

## Types

`napi build` auto-generates `index.d.ts`. Don't hand-edit; if you need
richer types wrap in TypeScript at `crates/pf-ts/ts/index.ts`.

## Errors

Map `pf_core::Error` via `napi::Error::from_reason` for v1; richer error
codes in v1.1.
