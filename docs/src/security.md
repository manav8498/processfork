# Security model

> Engineering source: [`SECURITY.md`](https://github.com/manav8498/processfork/blob/main/SECURITY.md).

Four threat categories:

1. **Semantic-rollback** (ACRFence, [arXiv 2603.20625](https://arxiv.org/abs/2603.20625)).
   Defended by the HMAC-chained effect ledger and the
   `Irreversible`-default replay policy.
2. **Snapshot secret leakage** — env vars / cookies. Defended by
   `--scrub-env <regex>` and a `.pfignore` (gitignore-syntax) on the
   FS layer.
3. **Supply-chain trust** — `.pfimg` artifacts are executable agent
   state. Defended by cosign-shaped manifest signing on push, verify
   on pull. v1 ships HMAC-SHA256 self-sign; cosign keyless lands in
   v1.1.
4. **Unsafe Rust** — only `pf-cache`'s GPU page-hashing is expected
   to need it; every `unsafe` block is reviewed and justified.

Reporting: open a private
[GitHub Security Advisory](https://github.com/manav8498/processfork/security/advisories/new).
