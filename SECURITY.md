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

## Advisories

### PF-SA-2026-001 — Path traversal on checkout (Zip Slip), v1.0.0–v1.0.2

**Severity:** High. **Fixed in:** v1.0.3.

A malicious `.pfimg` whose `fs.tree.v1` entries had `path` values
containing `..` segments or absolute paths could write outside the
target directory on `pf checkout` / `pf clone`. The `restore_tree`
function in `pf-world` did `staging.join(&entry.path)` with no
component validation. Symlink targets were also unchecked, so a
restored symlink could point outside the sandbox and a follow-up
write through the link would escape.

**Fix:** `safe_join()` validates every entry path component-by-
component (rejects `..`, absolute paths, Windows drive prefixes).
`check_symlink_target()` rejects absolute symlink targets and
relative targets whose component-walk depth ever goes negative
relative to the restore root. Three regression tests in
`crates/pf-world/src/fs.rs::tests`.

**Mitigation for v1.0.2 users:** only `pf checkout` / `pf clone` of
**untrusted** `.pfimg` files is exposed; locally-produced images are
unaffected (the capture path never emits `..`). Upgrade to v1.0.3.

Reported by Codex (anonymous reviewer) as part of the v1.0.2
production-readiness audit, 2026-05-06.

### PF-SA-2026-002 — Env-var secret leakage on `pf snapshot`, v1.0.0–v1.0.2

**Severity:** High. **Fixed in:** v1.0.3.

`pf snapshot` captured `std::env::vars()` verbatim into the world
layer's `env` blob; the documented `--scrub-env <regex>` flag was
never exposed on the CLI surface. Snapshots taken by operators with
`OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GITHUB_TOKEN` / etc. in
their shell environment leaked those secrets into the resulting
`.pfimg`.

**Fix:** v1.0.3 wires `--scrub-env <regex>` (repeatable) through
`pf snapshot` and `processfork.snapshot_filesystem()`. Recommended
baseline: `--scrub-env '(?i)token|secret|password|key'`. Regression
test in `crates/pf-cli/tests/cli_smoke.rs::snapshot_scrub_env_redacts_matching_keys`.

**Mitigation for v1.0.2 users:** **delete any `.pfimg` you pushed to
a registry while `OPENAI_API_KEY` (or similar) was in scope** — the
env blob is content-addressed so the secret is permanently in the
referenced blob until garbage-collected. Re-snapshot under v1.0.3
with `--scrub-env`.

## Out of scope (v1)

- Side-channel resistance.
- Post-restore confidential-computing attestation (v2 plan).
- Adversarial model-merge robustness beyond TIES+DARE defaults.
