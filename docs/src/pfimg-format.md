# The `.pfimg` format

OCI-compatible. Mediatype
`application/vnd.processfork.image.v1+json`. Top-level manifest is a
single JSON file referencing four layer blobs by SHA-256 content
hash:

```json
{
  "schemaVersion": 1,
  "mediaType":     "application/vnd.processfork.image.v1+json",
  "agent":   { "kind": "claude-code", "version": "0.4.2", "fingerprint": "…" },
  "model":   { "base": "sha256:…", "diff": "sha256:…" },
  "cache":   { "layout": "paged-batchinvariant-v1", "manifest": "sha256:…" },
  "world":   { "fs": "sha256:…", "env": "sha256:…", "procs": "sha256:…" },
  "effects": { "ledger": "sha256:…" },
  "trace":   { "messages": "sha256:…" },
  "createdAt": "2026-05-05T14:11:00Z",
  "parents": ["sha256:…"]
}
```

## Properties

- All blob content is SHA-256 content-addressed and zstd-19
  compressed at rest.
- Identical content across forks shares storage automatically (CAS
  dedup).
- Two parents iff the image was produced by a `pf merge`.
- Manifest's `createdAt` is the only field that changes between
  identical-content snapshots; the rest of the manifest digest
  reflects the actual layer state.

## Layer formats

Each layer's blob format has its own page:

- [Model layer](./layers/model.md) — `model.diff.v1`
- [Cache layer](./layers/cache.md) — `paged-batchinvariant-v1`
- [World layer](./layers/world.md) — `fs.tree.v1`, `env.v1`, `procs.{criu,unsupported}.v1`
- [Effects layer](./layers/effects.md) — `effects.ledger.v1`, `effects.merged.v1`
- Trace — JSONL of `{role, content}` messages.
