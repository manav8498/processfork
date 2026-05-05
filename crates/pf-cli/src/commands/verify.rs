// SPDX-License-Identifier: MIT
//! `pf verify` — re-hash every blob, fail on mismatch.

use std::path::Path;

use clap::Parser;
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

    println!("verified {total} blob(s); {bad} bad");
    if bad > 0 {
        return Err(CliError::Integrity(format!("{bad} blob(s) failed verification")).into());
    }
    Ok(())
}
