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

    /// Restore absolute-target symlinks verbatim. By default
    /// (v1.0.14) absolute symlinks are SKIPPED with a stderr
    /// warning and the rest of the tree restores normally — that
    /// matches what `tar`/`rsync` do, and the v1.0.3 "Zip Slip"
    /// CVE protection (PF-SA-2026-001) is unaffected because we
    /// never WRITE through the symlink, only choose whether to
    /// create it.
    ///
    /// Pass this flag when you genuinely need the absolute symlinks
    /// in the restored tree (e.g. `/var/log/agent` pointing at the
    /// production log directory). The operator explicitly
    /// acknowledges that anything later reading through the
    /// symlink may escape the sandbox.
    #[arg(long)]
    pub allow_absolute_symlinks: bool,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let cid =
        Digest256::parse(&args.cid).map_err(|e| CliError::BadInput(format!("bad cid: {e}")))?;
    let manifest = store.get_manifest(&cid)?;
    let blobs = store.blobs_arc();
    pf_world::restore_tree_with_options(
        &blobs,
        &manifest.world.fs,
        &args.into,
        pf_world::RestoreOptions {
            allow_absolute_symlinks: args.allow_absolute_symlinks,
        },
    )?;
    // Drop a `.pfcid` sentinel so a subsequent `pf snapshot --fs-root <into>`
    // automatically picks `cid` as its parent (closes the v1.0.2 audit
    // 'fork → edit → snapshot → merge breaks because no common ancestor'
    // finding without forcing every operator to remember --parent).
    let _ = std::fs::write(args.into.join(".pfcid"), cid.as_str().as_bytes());
    println!("✓ restored to {}", args.into.display());
    Ok(())
}
