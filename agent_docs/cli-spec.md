# CLI spec

`pf` is the user-facing entry point. Single static binary, <15 MB stripped.
Every subcommand prints `--help` describing exactly what it does. No
side-effects except those invoked.

## Global flags

```
-v, --verbose...           Increase tracing verbosity (-v, -vv, -vvv)
    --store <path>         CAS / index location (default $PF_STORE or ~/.processfork)
    --no-color             Disable ANSI colour
-h, --help                 Print help
-V, --version              Print version
```

## Subcommands

### `pf snapshot <agent-id> [--name N] [--exact] [--scrub-env REGEX]`

Capture a live agent into a `.pfimg`. Returns the new content-id.

```
$ pf snapshot claude-session-1 --name pre-refactor
✓ snapshot bafyreif3kxy…sjp9zq  (1.2 GB, 87 ms)
```

### `pf fork <CID> -n <count> [--explore HINT] [--name PREFIX]`

Spawn `count` divergent branches from a snapshot. Returns one CID per branch.

### `pf checkout <CID> [--into PATH]`

Restore a snapshot bit-exact on this machine.

### `pf merge <FROM> --into <INTO> [--alpha 0.5] [--replay-effects pure|idempotent|all|none] [--tool] [--continue]`

Three-way merge `FROM` into `INTO`.

### `pf push <CID> <TARGET>`

Push to a registry. Target schemes: `hf://`, `s3://`, `ipfs://`, `oci://`.

### `pf pull <SOURCE>`

Pull an image into the local store.

### `pf clone <SOURCE> [--into PATH]`

Pull and immediately check out.

### `pf log [--graph] [--max N]`

Show the snapshot DAG.

### `pf diff <A> <B> [--layers model,cache,world,effects,trace]`

Diff two images across selected layers.

### `pf status`

Show local store size, snapshot count, current checkout.

### `pf gc [--retain-recent N] [--dry-run]`

Garbage-collect unreferenced blobs.

### `pf verify [--deep]`

Re-hash every blob in the store; fail on mismatch.

## Exit codes

| code | meaning                                |
|------|----------------------------------------|
| 0    | success                                |
| 1    | user-recoverable error (bad input)     |
| 2    | not-yet-implemented (Phase 0 scaffold) |
| 3    | merge conflict needs resolution        |
| 4    | integrity failure (CAS hash mismatch)  |
| 70   | internal error (bug); please file      |

## Shell completions

`pf completions bash|zsh|fish|powershell` writes a completion script to
stdout. Wired via `clap_complete`.
