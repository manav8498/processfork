# World layer

The world is everything outside the model: filesystem, env vars, in-flight
subprocesses, and (optionally) an attached browser DOM.

## Filesystem

| OS      | Backend         | Snapshot mechanism                        |
|---------|-----------------|-------------------------------------------|
| Linux   | overlayfs       | snapshot the upperdir tree to CAS         |
| macOS   | APFS            | `clonefile(2)` for cheap CoW dir clones   |
| (other) | walk + tar      | fallback; slower, no CoW                  |

On capture: walk the mount, hash each file (SHA-256 of contents), CAS-store
those that are new, emit a tree manifest:

```json
{ "kind": "fs.tree.v1", "entries": [
  { "path": "src/main.rs", "mode": "0644", "size": 1234, "blob": "sha256:…" },
  { "path": ".git/HEAD",   "mode": "0644", "size": 45,   "blob": "sha256:…" },
  …
]}
```

On restore: rebuild the tree atomically into a tempdir, then `rename(2)` the
tempdir over the target path.

## Environment

A trivial JSON serialization of `std::env::vars()` plus `cwd`. `--scrub-env
<regex>` is applied here pre-seal.

## Subprocesses (Linux only via CRIU)

For an agent with attached subprocesses (postgres, dev-server, headless
browser), we shell out to `criu dump --tree <pid> --images-dir <tmp>` then CAS
the resulting checkpoint directory. Restore is `criu restore --images-dir`.

CRIU prerequisites:
- Linux kernel ≥4.0 with `CONFIG_CHECKPOINT_RESTORE=y`.
- `criu` binary in PATH (document install: `apt install criu` /
  `dnf install criu`).
- Capabilities: `cap_sys_admin` or running as root.

On macOS we **skip** subprocess capture and emit a warning entry into the
manifest's `procs` blob: `{"unsupported_on": "darwin", "warning": "subprocesses not captured"}`.

## Browser DOM (CDP)

For Playwright / Puppeteer attached browsers we connect to `--remote-
debugging-port` and capture:
- All open pages' URLs, scroll positions, viewport sizes.
- Per-page `Page.captureSnapshot` (MHTML).
- LocalStorage / SessionStorage / cookies (subject to `--scrub-env`).

On restore: spawn a fresh Chromium with the same flags, CDP-load each MHTML
page, and re-attach to the agent SDK.

## Capture order

1. Quiesce: signal the agent runtime that we're about to snapshot.
2. fsync mutable mounts.
3. Capture FS tree (parallel over file hashing).
4. Capture env (instant).
5. CRIU dump subprocesses (Linux).
6. CDP-capture browser pages (if attached).
7. Emit world-layer manifest.
8. Release quiesce.

Step 3 dominates wall-clock for large workspaces; we parallelize hashing with
`rayon` and skip files unchanged since the last snapshot (mtime+size pre-
filter, then content hash).
