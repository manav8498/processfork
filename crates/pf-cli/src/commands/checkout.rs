// SPDX-License-Identifier: MIT
//! `pf checkout` — restore the world-layer FS tree of an image.

use std::path::{Path, PathBuf};

use clap::Parser;
use pf_core::digest::Digest256;
use pf_core::store::PfStore;

use super::CliError;

#[derive(Debug, Parser)]
pub struct Args {
    /// Image content-id (`sha256:…`) to restore.
    pub cid: String,
    /// Where to materialise the world-layer FS tree. MUST NOT exist.
    #[arg(long)]
    pub into: PathBuf,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let cid =
        Digest256::parse(&args.cid).map_err(|e| CliError::BadInput(format!("bad cid: {e}")))?;
    let manifest = store.get_manifest(&cid)?;
    let blobs = store.blobs_arc();
    pf_world::restore_tree(&blobs, &manifest.world.fs, &args.into)?;
    println!("✓ restored to {}", args.into.display());
    Ok(())
}
