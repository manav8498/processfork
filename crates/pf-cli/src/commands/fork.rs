// SPDX-License-Identifier: MIT
//! `pf fork` — copy a manifest with new fingerprints to seed N divergent
//! branches.
//!
//! v1 ships the manifest-level fork: each new branch points at the same
//! layer blobs (CoW; no copy) but has a unique fingerprint and a single
//! `parents = [<source>]` entry. Spawning live diverging agents is the
//! responsibility of the integration adapters in Phase 10.

use std::path::Path;

use clap::Parser;
use pf_core::digest::Digest256;
use pf_core::manifest::AgentInfo;
use pf_core::store::PfStore;

use super::CliError;

#[derive(Debug, Parser)]
pub struct Args {
    /// Source content-id.
    pub cid: String,
    /// Number of branches to spawn.
    #[arg(short = 'n', long, default_value_t = 1)]
    pub count: u32,
    /// Optional steering hint, recorded in each branch's fingerprint.
    #[arg(long)]
    pub explore: Option<String>,
    /// Prefix prepended to each branch's fingerprint.
    #[arg(long, default_value = "fork")]
    pub name: String,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    if args.count == 0 {
        return Err(CliError::BadInput("--count must be ≥ 1".into()).into());
    }
    let store = PfStore::open(store_root)?;
    let parent_cid =
        Digest256::parse(&args.cid).map_err(|e| CliError::BadInput(format!("bad cid: {e}")))?;
    let parent = store.get_manifest(&parent_cid)?;

    for i in 0..args.count {
        let mut child = parent.clone();
        child.parents = vec![parent_cid.clone()];
        child.created_at = chrono::Utc::now();
        child.agent = AgentInfo {
            kind: parent.agent.kind.clone(),
            version: parent.agent.version.clone(),
            fingerprint: format!(
                "{}-{}{}{}",
                args.name,
                i,
                if args.explore.is_some() { ":" } else { "" },
                args.explore.as_deref().unwrap_or(""),
            ),
        };
        let child_cid = store.put_manifest(&child)?;
        println!("{child_cid}");
    }
    Ok(())
}
