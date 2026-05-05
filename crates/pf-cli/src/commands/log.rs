// SPDX-License-Identifier: MIT
//! `pf log` — print the snapshot DAG.

use std::path::Path;

use clap::Parser;
use pf_core::store::PfStore;

#[derive(Debug, Parser)]
pub struct Args {
    /// Render parent edges with simple ASCII graph chars.
    #[arg(long)]
    pub graph: bool,
    /// Maximum number of manifests to list.
    #[arg(long, default_value_t = 50)]
    pub max: usize,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let mut entries: Vec<_> = store.iter_manifests()?.collect();
    // Newest first.
    entries.sort_by_key(|(_, m)| std::cmp::Reverse(m.created_at));
    entries.truncate(args.max);

    if entries.is_empty() {
        println!("(no manifests in store)");
        return Ok(());
    }

    for (cid, m) in entries {
        let glyph = if args.graph {
            if m.parents.len() == 2 {
                "M"
            } else if m.parents.is_empty() {
                "*"
            } else {
                "│"
            }
        } else {
            ""
        };
        println!(
            "{glyph} {cid}  {}  agent={}@{}  parents={}",
            m.created_at.format("%Y-%m-%dT%H:%M:%S"),
            m.agent.kind,
            m.agent.version,
            m.parents.len(),
        );
    }
    Ok(())
}
