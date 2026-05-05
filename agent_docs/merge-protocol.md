# Merge protocol

`pf merge B -> A`: take the work that happened on branch B since common
ancestor X, and apply it to A. ProcessFork merges across all four layers
plus the trace, with each layer using the algorithm best suited to it.

## Common-ancestor discovery

Every image's manifest has a `parents[]` list. The `pf-merge` engine walks
both ancestor chains breadth-first and picks the lowest common ancestor.
Multi-parent merges (octopus) are NOT supported in v1; reject with a clear
error.

## Layer-by-layer

### Trace

Extract a "lessons learned" patch from B's trace by calling out to whatever
LLM the user has configured via `PF_SUMMARIZER` (default `claude-haiku-4-5`).
The summarizer prompt:

```
Given the following agent trace from branch B (diverged from common ancestor
X at <ts>), summarize what B *learned* in <= 4 sentences. Output ONLY the
summary as a system-message-formatted patch suitable for injection into a
sibling branch.
```

The output is appended as a system message into A's trace. **Only the
appended span is re-prefilled** — A's earlier KV-cache pages are reused.
This is the cache-layer payoff: merging doesn't cost a full prefill.

### World

Three-way file diff. For each path in `union(A.fs, B.fs, X.fs)`:

| A | B | X | outcome                                         |
|---|---|---|-------------------------------------------------|
| = | = | = | unchanged                                       |
| ≠ | = | X | A wins (B didn't touch)                         |
| = | ≠ | X | B wins (A didn't touch)                         |
| ≠ | ≠ | X (and A==B) | identical change, no conflict            |
| ≠ | ≠ | X (and A≠B)  | CONFLICT: write `<<<<<<<` markers, surface |

Conflicts surface in `pf merge --tool` exactly like git merge conflicts.
After resolution, `pf merge --continue` consumes the resolved files.

### Effects

NEVER replay irreversible. The merged image's effect ledger is the union of
A's and B's ledgers in causal order, with B's irreversible entries marked
`replayed: false, reason: "merged from sibling"`. The next call that depends
on one of B's facts gets the cached result.

`--replay-effects` overrides per-class:

```
pf merge B -> A --replay-effects=idempotent       # safe-by-construction
pf merge B -> A --replay-effects=all              # dangerous; warn
```

### Model

If both A and B have non-trivial weight diffs from X, apply TIES + DARE task
arithmetic:

```
Δ_merged = TIES_DARE(Δ_A, Δ_B, alpha=0.5, dare_p=0.7)
```

Surface as a soft warning ("merging weight diffs; review with `pf diff
--model`"). `--alpha` overrides the mixing coefficient.

## Conflict surfacing UX

`pf merge winner-3 -> main`:

```
Merging branch winner-3 into main (common ancestor: bafy…abc, depth 47).

  trace:    clean (3 sentences appended; re-prefill 47 tokens)
  world:    1 file conflicted
              src/server.rs   <<<<<<<  branch wrote `tokio::spawn`,
                                       main wrote `tokio::task::spawn_local`
  effects:  47 calls from winner-3 cached as facts
              (use `--replay-effects=all` to re-issue; not recommended)
  model:    weight diffs detected; applying TIES+DARE α=0.5
              base divergence: 0.012 cosine — within budget

Conflict in 1 file; resolve and run `pf merge --continue`.
```

## Determinism

Given the same A, B, X, summarizer model, and `PF_SEED`, merge output is
deterministic. The summarizer call is cached by `(A, B, X)` triple; identical
merges short-circuit.
