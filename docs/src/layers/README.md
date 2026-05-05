# The four layers

ProcessFork's job is to capture the simultaneous state of an agent
across four independent surfaces:

- [Model layer](./model.md) — weights and weight-diffs.
- [Cache layer](./cache.md) — paged KV cache.
- [World layer](./world.md) — filesystem, env, in-flight processes,
  optionally a browser DOM.
- [Effects layer](./effects.md) — append-only ledger of irreversible
  tool calls.

Plus the **trace** (chat + tool-call messages), captured in the same
manifest for typed three-way merge.

Each layer ships:

- A wire format (documented in the per-layer page).
- A capture API.
- A restore API.
- A merge primitive (in `pf-merge`).

The CAS layer below all four is content-addressed by SHA-256 and
zstd-19-compressed; identical content across forks shares storage
automatically.
