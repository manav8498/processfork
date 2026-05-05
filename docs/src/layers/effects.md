# Effects layer

> Engineering source: [`agent_docs/effects-layer.md`](https://github.com/manav8498/processfork/blob/main/agent_docs/effects-layer.md).

The append-only ledger of every external side-effect the agent has
caused. Defends against the **semantic-rollback attack** (ACRFence,
[arXiv 2603.20625](https://arxiv.org/abs/2603.20625)): an attacker
arranging that a fork "forgets" it sent the threatening email, then
tricks the merged-back branch into sending a second.

## Side-effect classes

| Class          | Default replay policy                               |
|----------------|-----------------------------------------------------|
| `pure`         | replay from cached `result_hash`                    |
| `idempotent`   | replay from cache OR re-call with same key (safe)   |
| `irreversible` | NEVER replay; surface as cached fact                |
| `network-only` | replay from cache; warn on stale TTL                |

Classification is declared by the tool author at registration time.

## HMAC chain

Each ledger entry carries
`HMAC-SHA256(session_secret, prev_entry_hash || this_entry_minus_hmac)`,
hex-encoded. Tampering with any entry breaks the chain at that index.

## API

```rust
use pf_effects::{Ledger, SessionSecret, ToolProxy, SideEffectClass};

let ledger = Ledger::new(SessionSecret::generate()?);
let proxy  = ToolProxy::new(ledger);
proxy.register("send_email", Arc::new(SendEmail), SideEffectClass::Irreversible);
proxy.invoke("send_email", &args)?;
```

## Replay policy presets

```rust
use pf_effects::ReplayPolicy;

ReplayPolicy::default()    // never re-issue Irreversible
ReplayPolicy::strict()     // surface everything except Pure
ReplayPolicy::aggressive() // re-issue Irreversible w/ NEW keys (--replay-effects=all)
```
