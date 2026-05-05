# Security policy

## Reporting a vulnerability

Please open a private [GitHub Security Advisory][adv] for this
repository instead of a public issue. The maintainer is notified
immediately and the conversation stays confidential until a fix is
ready.

[adv]: https://github.com/manav8498/processfork/security/advisories/new

We aim to acknowledge within 72 hours and ship a fix or mitigation
within 30 days for high-severity issues.

## Threat model

ProcessFork's threat model focuses on four categories:

### 1. Semantic-rollback attack

An attacker arranges that an agent's `pf checkout` "forgets" an
irreversible side effect (e.g., the agent already sent a threatening
email), then manipulates the resumed agent into re-issuing it.
ACRFence (arXiv 2603.20625) formalizes this.

**Defense:** the effects-layer ledger is HMAC-chained per session;
each entry carries `session_hmac = HMAC(secret, prev_hash ||
this_entry_minus_hmac)`. A restored agent that drops or reorders
ledger entries breaks the chain. Replay policy default is `none` for
`irreversible` — restored agents see the prior result as a cached
**fact**, not as an opportunity to re-issue.

### 2. Snapshot secret leakage

Snapshots may contain credentials in env vars, in-memory tokens,
browser cookies, etc.

**Defense:** `pf snapshot --scrub-env <regex>` redacts matching env
vars pre-seal. World-layer FS capture honours a `.pfignore` file
(gitignore syntax). Browser cookies require explicit opt-in via
`--include-cookies`. Push to a public registry without a key
explicitly fails.

### 3. Supply-chain trust

`.pfimg` artifacts are executable agent state; pulling a malicious
image is analogous to pulling a malicious container.

**Defense:** every push signs the manifest with cosign (keyless
Sigstore by default). Pull verifies before any blob touches disk.
`pf pull --insecure` opts out, with a loud warning.

### 4. Unsafe Rust

The `pf-cache` crate is the only place we expect FFI / unsafe in v1
(GPU-side page hashing). Every `unsafe` block carries a comment
explaining why and what invariants it relies on. Reviewed in every
PR.

## Out of scope (v1)

- Side-channel resistance.
- Post-restore confidential-computing attestation (v2 plan).
- Adversarial model-merge robustness beyond TIES+DARE defaults.
