// SPDX-License-Identifier: MIT
//! `pf merge-finalize` — capture the resolved workdir as a new
//! single-parent image whose parent is the merged-CID. Pairs with
//! `pf merge-resolve`.
//!
//! By default the command refuses to finalize if any file in the
//! workdir still contains Git-style conflict markers — pass
//! `--force` to override (the operator may legitimately want
//! literal `<<<<<<<` content in their tree).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::Manifest;
use pf_core::store::PfStore;

use super::CliError;
use super::merge_resolve;

#[derive(Debug, Parser)]
pub struct Args {
    /// CID of the conflicted merge image — passed to `pf
    /// merge-resolve` earlier. Becomes the single parent of the
    /// finalized image.
    pub cid: String,

    /// Working directory the operator edited. Must contain a
    /// resolved tree (no conflict markers) unless `--force`.
    #[arg(long)]
    pub workdir: PathBuf,

    /// Skip the conflict-marker scan and finalize the workdir as-is.
    /// Use when your tree legitimately contains literal `<<<<<<<`
    /// content (e.g. test fixtures that exercise the merge engine).
    #[arg(long)]
    pub force: bool,

    /// Optional human-readable name written into the new manifest's
    /// `agent.fingerprint`. Defaults to `pf-cli-finalize`.
    #[arg(long)]
    pub name: Option<String>,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let blobs: Arc<dyn BlobStore> = store.blobs_arc();

    if !args.workdir.exists() {
        return Err(CliError::BadInput(format!(
            "--workdir does not exist: {}",
            args.workdir.display()
        ))
        .into());
    }

    let parent_cid =
        Digest256::parse(&args.cid).map_err(|e| CliError::BadInput(format!("bad CID: {e}")))?;
    let parent = store.get_manifest(&parent_cid)?;

    if !args.force {
        let still_conflicting = merge_resolve::scan_conflict_markers(&args.workdir)?;
        if !still_conflicting.is_empty() {
            let listed: String = still_conflicting
                .iter()
                .map(|p| format!("  {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(CliError::MergeConflict(format!(
                "{} file(s) still contain conflict markers:\n{listed}\n\
                 Resolve by hand or pass --force to finalize as-is.",
                still_conflicting.len()
            ))
            .into());
        }
    }

    // Re-walk the (now-resolved) workdir to produce a fresh FS
    // digest. The other layers are inherited from the merged image:
    //  - cache / model: byte-for-byte (those layers don't carry the
    //    conflict markers; only world.fs does).
    //  - effects: the merged ledger already represents the union of
    //    A's and B's tool calls, HMAC-rechain happened at merge
    //    time. We pass it through.
    //  - trace: same — the merge engine emitted the merged trace.
    //  - env / procs: inherited from the merged image (which itself
    //    inherited from one of the branches per the merge rules).
    let new_fs = pf_world::WalkFsCapture::new(&args.workdir).capture(&blobs)?;

    let fingerprint = args.name.unwrap_or_else(|| "pf-cli-finalize".into());
    let manifest = Manifest {
        schema_version: 1,
        media_type: parent.media_type.clone(),
        agent: pf_core::manifest::AgentInfo {
            kind: parent.agent.kind.clone(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            fingerprint,
        },
        model: parent.model.clone(),
        cache: parent.cache.clone(),
        world: pf_core::manifest::WorldLayer {
            fs: new_fs,
            env: parent.world.env.clone(),
            procs: parent.world.procs.clone(),
        },
        effects: parent.effects.clone(),
        trace: parent.trace.clone(),
        created_at: chrono::Utc::now(),
        parents: vec![parent_cid],
    };
    let cid = store.put_manifest(&manifest)?;
    println!("finalized: {cid}");
    println!("parent   : {}", args.cid);
    println!(
        "fs_digest: {}  (new — re-walked from workdir post-resolution)",
        manifest.world.fs
    );
    Ok(())
}
