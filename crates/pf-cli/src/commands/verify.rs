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
}

pub fn run(store_root: &Path, _args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let blobs_dir = store.root().join("blobs").join("sha256");
    let mut total: u64 = 0;
    let mut bad: u64 = 0;

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
    let mut chains_ok: u64 = 0;
    let mut chains_bad: u64 = 0;
    let mut chains_skipped: u64 = 0;
    for (cid, m) in store.iter_manifests()? {
        let Ok(ledger_bytes) = store.blobs().get(&m.effects.ledger) else {
            chains_skipped += 1;
            continue;
        };
        match validate_ledger_chain(&ledger_bytes) {
            ChainStatus::Ok => chains_ok += 1,
            ChainStatus::Bad(reason) => {
                chains_bad += 1;
                eprintln!("BAD ledger in manifest {cid}: {reason}");
            }
            ChainStatus::Skipped => chains_skipped += 1,
        }
    }

    println!(
        "verified {total} blob(s); {bad} bad. effects ledgers: {chains_ok} ok, \
         {chains_bad} bad, {chains_skipped} skipped (no embedded secret)"
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
    Ok(())
}

enum ChainStatus {
    Ok,
    Bad(String),
    /// No embedded session secret — ACRFence verification needs the
    /// operator to bring the secret out-of-band (not yet wired in
    /// `pf verify`); the chain blob is structurally well-formed.
    Skipped,
}

/// Validate an `effects.ledger.v1` blob's HMAC chain. The blob's
/// header may carry `session_secret_hex` (tamper-detection mode); if
/// present we verify with that secret. If absent we return `Skipped`.
fn validate_ledger_chain(bytes: &[u8]) -> ChainStatus {
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
        // Empty ledger — nothing to verify; structurally OK.
        return ChainStatus::Ok;
    }
    let secret_hex = match header.get("session_secret_hex").and_then(|v| v.as_str()) {
        Some(h) => h.to_owned(),
        None => return ChainStatus::Skipped,
    };
    let secret_bytes = match hex::decode(secret_hex.trim()) {
        Ok(b) => b,
        Err(e) => return ChainStatus::Bad(format!("bad session_secret_hex: {e}")),
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
        Ok(()) => ChainStatus::Ok,
        Err(e) => ChainStatus::Bad(format!("HMAC chain: {e}")),
    }
}
