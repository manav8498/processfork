# Effects layer

The append-only ledger of every external side-effect the agent has caused.
Defends against the **semantic-rollback attack** (ACRFence,
arXiv 2603.20625): an attacker arranging that a fork "forgets" it sent the
threatening email, then tricks the merged-back branch into sending a second.

## Ledger entry shape

```json
{
  "ts": "2026-05-05T14:11:00.123Z",
  "tool_id": "send_email",
  "args_hash": "sha256:…",
  "idempotency_key": "01J…ULID",
  "result_hash":  "sha256:…",
  "side_effect_class": "irreversible",
  "session_hmac": "sha256:…"
}
```

`session_hmac` is HMAC(session-secret, prev_entry_hash || this_entry_minus_hmac).
It chains entries — tampering with any earlier entry invalidates every later
HMAC.

## Side-effect classes

| Class          | Default replay policy                               |
|----------------|-----------------------------------------------------|
| `pure`         | replay from cached `result_hash`                    |
| `idempotent`   | replay from cache OR re-call with same key (safe)   |
| `irreversible` | NEVER replay; surface as cached fact                |
| `network-only` | replay from cache; warn on stale TTL                |

Classification is declared by the tool author at registration time, not
inferred. Misclassification is a contract violation by the tool, not the
ledger.

## Replay-or-fork policy

When restoring an image, the SDK iterates the ledger:

```
for entry in ledger:
    match (entry.side_effect_class, --replay-effects flag):
      (Pure | Idempotent | NetworkOnly, *)        => inject cached result into next call
      (Irreversible, --replay-effects=false)      => inject cached result + flag as fact
      (Irreversible, --replay-effects=true)       => mint new idempotency_key, re-issue
```

## Conformance fuzzer

`crates/pf-effects/tests/fuzz_replay.rs` (proptest) generates 1000 random
ledger sequences and asserts:

1. Replay never re-issues an `irreversible` call without `--replay-effects`.
2. Idempotency keys are unique within a session.
3. HMAC chain validates.
4. Forking does not duplicate any `irreversible` effect.

## Tool registration

Tools register with a `ToolProxy`:

```rust
proxy.register("send_email", SendEmailTool, SideEffectClass::Irreversible);
proxy.register("read_file",  ReadFileTool,  SideEffectClass::Pure);
proxy.register("http_get",   HttpGetTool,   SideEffectClass::NetworkOnly);
```

The proxy intercepts every call, computes `args_hash`, mints an
`idempotency_key`, runs the tool, hashes the result, appends to the ledger.
