# World layer

> Engineering source: [`agent_docs/world-layer.md`](https://github.com/processfork/processfork/blob/main/agent_docs/world-layer.md).

The world is everything outside the model: filesystem, env vars,
in-flight subprocesses, and (optionally) an attached browser DOM.

## Filesystem

| OS    | Backend          | Snapshot mechanism                       |
|-------|------------------|------------------------------------------|
| Linux | overlayfs        | snapshot the upperdir tree to CAS        |
| macOS | APFS             | `clonefile(2)` for cheap CoW dir clones  |
| any   | walk + tar       | fallback; slower, no CoW                 |

Capture via `pf_world::WalkFsCapture`. Restore via
`pf_world::restore_tree` — atomic rebuild into a sibling temp dir,
then `rename(2)` over the destination.

## Env

`pf_world::EnvCapture` serializes `std::env::vars()` + cwd into a
sorted `BTreeMap` (deterministic digest). Optional regex scrub:

```rust
EnvCapture::new()
    .scrub("(?i)secret|token|password")?
    .capture(&blobs)?;
```

## Procs (Linux only)

`pf_world::ProcsCapture` shells out to `criu dump --tree <pid>`. On
macOS we emit a `procs.unsupported.v1` placeholder so restore can
warn cleanly.

## Browser DOM

Captured via Chrome DevTools Protocol when a Playwright / Puppeteer
browser is attached. Phase-1.1 deliverable; the manifest carries an
empty `dom` blob in v1.0.
