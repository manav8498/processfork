# Security policy

> Source: [`SECURITY.md`](https://github.com/manav8498/processfork/blob/main/SECURITY.md).

## Reporting

Email **security@processfork.dev** or open a GitHub Security
Advisory. We acknowledge within 72 hours; we ship a fix or
mitigation within 30 days for high-severity issues.

## Threat model

See [Security model](./security.md) for the four threat categories
ProcessFork explicitly defends against (semantic-rollback, snapshot
secret leakage, supply-chain trust, unsafe Rust audit) and what's
out of scope for v1.

## Cryptography

- Manifest signing: HMAC-SHA256 self-signed (v1.0); cosign keyless
  via Sigstore Fulcio (v1.1).
- Effect-ledger HMAC chain: `ring::hmac` with SHA-256, per-session
  secret.
- Content addressing: SHA-256 OCI-style throughout.
