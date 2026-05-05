# `pf` CLI overview

> Engineering source: [`agent_docs/cli-spec.md`](https://github.com/manav8498/processfork/blob/main/agent_docs/cli-spec.md).

Single static binary, <15 MB stripped. Twelve subcommands; ten wired
in v1.0, three deferred to Phase 9 (push / pull / clone — wired in
this release).

## Global flags

```
-v, --verbose...       Increase tracing verbosity (-v, -vv, -vvv)
    --store <path>     CAS / index location (env $PF_STORE; default ~/.processfork)
    --no-color         Disable ANSI colour
-h, --help             Print help
-V, --version          Print version
```

## Subcommands

| command       | what it does                                              |
|---------------|-----------------------------------------------------------|
| `snapshot`    | Capture FS sandbox + chat trace into a `.pfimg`           |
| `fork`        | Branch one or more divergent live agents from a snapshot  |
| `checkout`    | Restore the world-layer FS tree of an image               |
| `merge`       | Three-way merge B into A                                  |
| `push`        | Push to a registry (`file://`, `hf://`, `s3://`, `ipfs://`) |
| `pull`        | Pull from a registry                                      |
| `clone`       | Pull + checkout                                           |
| `log`         | Show the snapshot DAG                                     |
| `diff`        | Diff two images per layer                                 |
| `status`      | Show local store summary                                  |
| `gc`          | Mark-and-sweep over orphan blobs                          |
| `verify`      | Re-hash every blob; fail on mismatch                      |
| `completions` | Emit shell completions                                    |

## Exit codes

| code | meaning                                  |
|------|------------------------------------------|
| 0    | success                                  |
| 1    | user-recoverable error (bad input)       |
| 2    | not-yet-implemented (feature-gated)      |
| 3    | merge conflict needs resolution          |
| 4    | integrity failure (CAS hash mismatch, sig) |

Run `pf <subcommand> --help` for per-command flags.
