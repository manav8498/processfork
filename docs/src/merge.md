# Three-way merge protocol

> Engineering source: [`agent_docs/merge-protocol.md`](https://github.com/processfork/processfork/blob/main/agent_docs/merge-protocol.md).

`pf merge B --into A` takes the work that happened on branch B since
common ancestor X, and applies it to A. ProcessFork merges across all
four layers plus the trace, with each layer using the algorithm best
suited to it.

## Common-ancestor discovery

Every image's manifest has a `parents[]` list. The `pf-merge` engine
walks both ancestor chains breadth-first and picks the lowest common
ancestor. Multi-parent merges (octopus) are NOT supported in v1;
`pf merge` rejects them with a clear error.

## Per-layer primitives

| Layer       | Merge primitive                                                        |
|-------------|------------------------------------------------------------------------|
| **Trace**   | LLM-summarised "lessons" patch, re-prefilled into target               |
| **World**   | Git-style 3-way file diff with `<<<<<<<` conflict markers              |
| **Effects** | NEVER replay irreversible; cached as facts (or `--replay-effects`)     |
| **Model**   | TIES + DARE on weight-diff vectors; `α=0.5` default                    |

## Conflict surfacing

```
Merging sha256:f71b… into sha256:7974…
  ancestor : sha256:7974…
  trace    : clean (37 chars summary; ~9 re-prefill tokens)
  world    : 1 file conflicted
              src/server.rs   <<<<<<<  branch wrote `tokio::spawn`,
                                       main wrote `tokio::task::spawn_local`
  effects  : 47 calls from sibling cached as facts
              (use `--replay-effects=all` to re-issue; not recommended)
  model    : weight diffs detected; applying TIES + DARE α=0.5

Conflict in 1 file; resolve and run `pf merge --continue`. (v1.1)
```

`pf merge` exits 3 on conflict (per the CLI spec), 0 on clean.

## Determinism

Given the same `(A, B, X)`, summariser model, and `PF_SEED`, merge
output is deterministic. The summariser call is cached by `(A, B, X)`
triple; identical merges short-circuit.
