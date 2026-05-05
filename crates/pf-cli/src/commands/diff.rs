// SPDX-License-Identifier: MIT
//! `pf diff` — diff two images across all four layers.

use std::path::Path;

use clap::Parser;
use pf_core::digest::Digest256;
use pf_core::store::PfStore;

use super::CliError;

#[derive(Debug, Parser)]
pub struct Args {
    /// First image.
    pub a: String,
    /// Second image.
    pub b: String,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let a = Digest256::parse(&args.a).map_err(|e| CliError::BadInput(format!("bad cid a: {e}")))?;
    let b = Digest256::parse(&args.b).map_err(|e| CliError::BadInput(format!("bad cid b: {e}")))?;
    let ma = store.get_manifest(&a)?;
    let mb = store.get_manifest(&b)?;

    println!("--- {a}");
    println!("+++ {b}");
    diff_pair("agent.kind", &ma.agent.kind, &mb.agent.kind);
    diff_pair("agent.version", &ma.agent.version, &mb.agent.version);
    diff_pair(
        "agent.fingerprint",
        &ma.agent.fingerprint,
        &mb.agent.fingerprint,
    );
    diff_pair("model.base", ma.model.base.as_str(), mb.model.base.as_str());
    diff_pair("model.diff", ma.model.diff.as_str(), mb.model.diff.as_str());
    diff_pair("cache.layout", &ma.cache.layout, &mb.cache.layout);
    diff_pair(
        "cache.manifest",
        ma.cache.manifest.as_str(),
        mb.cache.manifest.as_str(),
    );
    diff_pair("world.fs", ma.world.fs.as_str(), mb.world.fs.as_str());
    diff_pair("world.env", ma.world.env.as_str(), mb.world.env.as_str());
    diff_pair(
        "world.procs",
        ma.world.procs.as_str(),
        mb.world.procs.as_str(),
    );
    diff_pair(
        "effects.ledger",
        ma.effects.ledger.as_str(),
        mb.effects.ledger.as_str(),
    );
    diff_pair(
        "trace.messages",
        ma.trace.messages.as_str(),
        mb.trace.messages.as_str(),
    );
    Ok(())
}

fn diff_pair(label: &str, a: &str, b: &str) {
    if a == b {
        println!("  {label} = {a}");
    } else {
        println!("- {label} = {a}");
        println!("+ {label} = {b}");
    }
}
