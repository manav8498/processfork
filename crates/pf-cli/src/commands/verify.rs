// SPDX-License-Identifier: MIT
//! `pf verify` — re-hash every blob, validate every effects.ledger
//! HMAC chain, fail on the first mismatch.

use std::path::Path;

use clap::Parser;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::store::PfStore;

use super::CliError;

#[derive(Debug, Parser)]
pub struct Args {
    /// Verify every blob (default behaviour); included for spec
    /// compatibility — there's no "shallow" mode in v1.
    #[arg(long)]
    pub deep: bool,

    /// Operator-supplied session secret (hex) for true-ACRFence
    /// HMAC-chain validation. Pair this with snapshots that were
    /// written with `PF_SESSION_SECRET=<same-hex> pf snapshot ...`
    /// (the "real ACRFence" mode that does NOT embed the secret in
    /// the blob header).
    ///
    /// v1.0.15 audit fix: prior versions silently skipped HMAC-chain
    /// validation when `session_secret_hex` was absent from the
    /// header — i.e. exactly the case where the operator was running
    /// in true-ACRFence mode. The chain WAS verifiable; we just
    /// hadn't taken the secret as input. Now we do.
    ///
    /// Precedence: this flag > `PF_SESSION_SECRET` env var > embedded
    /// header secret. When both an operator secret and an embedded
    /// secret are present, the operator secret wins (the embedded
    /// one is by definition tamperable since it lives in the same
    /// blob as the entries).
    #[arg(long, env = "PF_SESSION_SECRET")]
    pub session_secret_hex: Option<String>,

    /// Treat ledgers with no available session secret (neither
    /// embedded nor supplied) as a verification failure rather than
    /// a skip. Use this in CI to catch ledgers that were written
    /// without the v1.0.7 HMAC chain wiring.
    #[arg(long)]
    pub fail_on_unverifiable_ledgers: bool,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let blobs_dir = store.root().join("blobs").join("sha256");
    let mut total: u64 = 0;
    let mut bad: u64 = 0;

    // Decode the operator-supplied secret once so per-ledger calls
    // are zero-cost.
    let operator_secret: Option<Vec<u8>> = match args.session_secret_hex.as_deref() {
        Some(s) if !s.trim().is_empty() => match hex::decode(s.trim()) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                return Err(CliError::BadInput(format!(
                    "--session-secret-hex / PF_SESSION_SECRET: {e}"
                ))
                .into());
            }
        },
        _ => None,
    };

    if blobs_dir.exists() {
        for shard in std::fs::read_dir(&blobs_dir)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for blob in std::fs::read_dir(shard.path())? {
                let blob = blob?;
                let name = blob.file_name().to_string_lossy().to_string();
                let hex = name.strip_suffix(".zst").unwrap_or(&name);
                let Ok(d) = Digest256::parse(&format!("sha256:{hex}")) else {
                    eprintln!("WARN: skipping non-blob file {}", blob.path().display());
                    continue;
                };
                total += 1;
                // BlobStore::get re-hashes on read; use that as our verifier.
                if let Err(e) = store.blobs().get(&d) {
                    bad += 1;
                    eprintln!("BAD  {d}: {e}");
                }
            }
        }
    }

    // v1.0.7 audit fix (#34): walk every manifest, find its effects
    // ledger, and validate the HMAC chain. Prior versions accepted
    // any well-formed JSONL; tampered ledgers (entry edited / deleted
    // / reordered) were silently accepted.
    //
    // v1.0.15 audit fix: also accept an operator-supplied secret
    // (--session-secret-hex / PF_SESSION_SECRET) so true-ACRFence
    // mode (secret kept out-of-band; not embedded in the blob)
    // actually validates instead of silently being skipped.
    let mut chains_ok: u64 = 0;
    let mut chains_bad: u64 = 0;
    let mut chains_skipped: u64 = 0;
    let mut chains_verified_with_operator_secret: u64 = 0;
    for (cid, m) in store.iter_manifests()? {
        let Ok(ledger_bytes) = store.blobs().get(&m.effects.ledger) else {
            chains_skipped += 1;
            continue;
        };
        match validate_ledger_chain(&ledger_bytes, operator_secret.as_deref()) {
            ChainStatus::Ok {
                used_operator_secret,
            } => {
                chains_ok += 1;
                if used_operator_secret {
                    chains_verified_with_operator_secret += 1;
                }
            }
            ChainStatus::Bad(reason) => {
                chains_bad += 1;
                eprintln!("BAD ledger in manifest {cid}: {reason}");
            }
            ChainStatus::Skipped => chains_skipped += 1,
        }
    }

    let skipped_note = if operator_secret.is_some() {
        "skipped (header missing kind=effects.ledger.v1)"
    } else {
        "skipped (no operator secret + no embedded secret)"
    };
    println!(
        "verified {total} blob(s); {bad} bad. effects ledgers: {chains_ok} ok \
         ({chains_verified_with_operator_secret} via operator secret), \
         {chains_bad} bad, {chains_skipped} {skipped_note}"
    );
    if bad > 0 {
        return Err(CliError::Integrity(format!("{bad} blob(s) failed verification")).into());
    }
    if chains_bad > 0 {
        return Err(CliError::Integrity(format!(
            "{chains_bad} effects ledger HMAC chain(s) failed verification"
        ))
        .into());
    }
    if args.fail_on_unverifiable_ledgers && chains_skipped > 0 {
        return Err(CliError::Integrity(format!(
            "{chains_skipped} effects ledger(s) had no verifiable HMAC chain \
             (no embedded secret + no --session-secret-hex / PF_SESSION_SECRET); \
             pass the operator secret or drop --fail-on-unverifiable-ledgers"
        ))
        .into());
    }
    Ok(())
}

enum ChainStatus {
    Ok {
        /// True if the chain was verified using the operator-supplied
        /// secret rather than the embedded one. v1.0.15 telemetry —
        /// surfaced in the verify line so the operator can confirm
        /// they hit the real-ACRFence path.
        used_operator_secret: bool,
    },
    Bad(String),
    /// Neither an operator secret nor an embedded secret is available
    /// for this ledger; the chain blob is structurally well-formed
    /// but the HMAC chain cannot be verified. Use
    /// `--fail-on-unverifiable-ledgers` to upgrade this to a hard
    /// failure.
    Skipped,
}

/// Validate an `effects.ledger.v1` blob's HMAC chain.
///
/// Secret precedence (highest → lowest):
///   1. `operator_secret` (passed by caller, originating from
///      `--session-secret-hex` or `PF_SESSION_SECRET`). Wins because
///      true ACRFence requires the operator to keep the secret OUT
///      of the blob; trusting only the embedded one means an
///      attacker who rewrites the blob can also re-sign it.
///   2. `header.session_secret_hex` (tamper-detection mode), if
///      present.
///   3. None → `Skipped`.
///
/// v1.0.15 audit fix: prior versions only checked `(2)` and silently
/// skipped chains written in real-ACRFence mode (no embedded
/// secret) — exactly the case where the operator most needed
/// validation. Now `(1)` plumbs the operator secret in.
fn validate_ledger_chain(bytes: &[u8], operator_secret: Option<&[u8]>) -> ChainStatus {
    use pf_core::cas::MemBlobStore;
    use pf_effects::ledger::{Ledger, SessionSecret};

    let mut split = bytes.splitn(2, |b| *b == b'\n');
    let header_bytes = split.next().unwrap_or(&[]);
    let header: serde_json::Value = match serde_json::from_slice(header_bytes) {
        Ok(v) => v,
        Err(_) => return ChainStatus::Skipped, // not a v1 ledger blob
    };
    if header.get("kind").and_then(|v| v.as_str()) != Some("effects.ledger.v1") {
        return ChainStatus::Skipped;
    }
    let entries_count = header
        .get("entries")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if entries_count == 0 {
        // Empty ledger — nothing to verify; structurally OK. We
        // didn't need either secret to reach this verdict, so
        // don't claim "via operator secret" in the telemetry.
        return ChainStatus::Ok {
            used_operator_secret: false,
        };
    }

    // Pick the secret per the precedence above.
    let (secret_bytes, used_operator_secret) = if let Some(op) = operator_secret {
        (op.to_vec(), true)
    } else {
        let Some(embedded_hex) = header.get("session_secret_hex").and_then(|v| v.as_str()) else {
            return ChainStatus::Skipped;
        };
        let bytes = match hex::decode(embedded_hex.trim()) {
            Ok(b) => b,
            Err(e) => return ChainStatus::Bad(format!("bad session_secret_hex: {e}")),
        };
        (bytes, false)
    };
    let secret = SessionSecret::new(secret_bytes);

    // Round-trip the bytes through a MemBlobStore so we can call the
    // pf-effects-native `Ledger::deserialize` + `verify()` instead of
    // re-implementing the chain math here (which kept this file out
    // of sync with the core in v1.0.7's first cut).
    let mem = MemBlobStore::new();
    let digest = match mem.put(bytes) {
        Ok(d) => d,
        Err(e) => return ChainStatus::Bad(format!("MemBlobStore put: {e}")),
    };
    let ledger = match Ledger::deserialize(&mem, &digest, secret) {
        Ok(l) => l,
        Err(e) => return ChainStatus::Bad(format!("ledger deserialize: {e}")),
    };
    match ledger.verify() {
        Ok(()) => ChainStatus::Ok {
            used_operator_secret,
        },
        Err(e) => ChainStatus::Bad(format!("HMAC chain: {e}")),
    }
}
