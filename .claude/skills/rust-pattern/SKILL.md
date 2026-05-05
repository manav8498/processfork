---
name: rust-pattern
description: ProcessFork's Rust idioms — Result propagation, tokio async, tracing instrumentation, error typing.
---

# Rust patterns

## Errors

- Library crates: `thiserror::Error`-derived enum named `Error`, type alias
  `pub type Result<T> = std::result::Result<T, Error>;`. Variants tagged with
  `#[error("…")]`.
- Binary crates (`pf-cli` only): `anyhow::Result` is acceptable at the
  command-handler boundary. Internals still use typed errors.
- Never `unwrap()` / `expect()` outside `#[cfg(test)]` and `examples/`.
- Use `?` everywhere. If you find yourself writing `match res { Err(e) =>
  return Err(e.into()), Ok(v) => v }`, write `res?` instead.

## Async

- `tokio` multi-thread runtime; `#[tokio::main]` only in binaries.
- Use `tokio::task::JoinSet` for fan-out/fan-in instead of `spawn` + `await`
  loops.
- `tokio::fs` for I/O paths in async contexts; `std::fs` only in clearly-
  sync contexts.

## Tracing

- `use tracing::{debug, info, warn, error, instrument};`
- Annotate every `pub` async function with `#[instrument(skip(self))]` or
  similar (skip large args).
- Spans MUST carry meaningful field names: `tracing::info!(image=%cid,
  "snapshot complete")`. Never log opaque `Debug` structs at info level.

## API surface

- Every `pub` symbol gets a doc-comment that compiles as a doctest where
  meaningful. `cargo test --doc` runs these in CI.
- Prefer borrowed parameters (`&str`, `&[u8]`, `&Path`); accept `impl
  AsRef<Path>` only at the outermost API boundary.
- Newtype the digest types — never expose raw `String` for content IDs.

## Workspace lints

- `pedantic` is on, `module_name_repetitions` and `must_use_candidate` are
  off. Fix every other clippy warning before committing.
