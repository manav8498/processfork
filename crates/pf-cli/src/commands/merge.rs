// SPDX-License-Identifier: MIT
//! `pf merge` — three-way merge B into A.

use std::path::Path;

use clap::Parser;
use pf_core::digest::Digest256;
use pf_core::store::PfStore;
use pf_merge::{MergeOptions, MergeOutcome, StubSummarizer};

use super::CliError;

#[derive(Debug, Parser)]
pub struct Args {
    /// Source branch (B) to merge IN.
    pub from: String,
    /// Target branch (A) to merge INTO.
    #[arg(long)]
    pub into: String,
    /// Mixing coefficient for the model-layer TIES merge.
    #[arg(long, default_value_t = 0.5)]
    pub alpha: f32,
    /// DARE drop probability.
    #[arg(long, default_value_t = 0.7)]
    pub dare_p: f64,
    /// PRNG seed.
    #[arg(long, default_value_t = 0)]
    pub seed: u64,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let from = Digest256::parse(&args.from)
        .map_err(|e| CliError::BadInput(format!("bad --from cid: {e}")))?;
    let into = Digest256::parse(&args.into)
        .map_err(|e| CliError::BadInput(format!("bad --into cid: {e}")))?;
    let opts = MergeOptions {
        alpha: args.alpha,
        dare_p: args.dare_p,
        seed: args.seed,
        replay_with_new_key: Vec::new(),
    };
    // Merge order is "B into A": engine signature is merge(store, A, B, …).
    let report = pf_merge::merge(&store, &into, &from, &StubSummarizer, &opts, None)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("Merging {} into {}", short(&args.from), short(&args.into));
    println!("  ancestor : {}", report.ancestor);
    println!(
        "  trace    : clean ({} chars summary; ~{} re-prefill tokens)",
        report.trace.injected_summary.len(),
        report.trace.re_prefill_token_estimate,
    );
    println!(
        "  world    : {} ({} clean paths; {} conflicts)",
        match report.per_layer.world {
            MergeOutcome::Clean => "clean",
            MergeOutcome::Conflicted => "CONFLICT",
            MergeOutcome::Skipped => "skipped",
        },
        report.world.clean_paths,
        report.world.conflicts.len(),
    );
    for c in &report.world.conflicts {
        println!("    <<<<<<<  {}", c.path);
    }
    println!("  effects  : merged blob = {}", report.effects);
    println!(
        "  model    : {}",
        if report.model.kind_mismatch {
            "kind mismatch (kept A)"
        } else if report.model.applied_task_arithmetic {
            "applied TIES + DARE"
        } else {
            "trivial"
        }
    );
    println!("merged   : {}", report.merged_cid);

    if report.overall == MergeOutcome::Conflicted {
        return Err(CliError::MergeConflict(format!(
            "{} conflict(s); resolve and run `pf merge --continue` (planned for v1.1)",
            report.world.conflicts.len()
        ))
        .into());
    }
    Ok(())
}

fn short(cid: &str) -> String {
    if cid.len() > 16 {
        format!("{}…", &cid[..16])
    } else {
        cid.to_string()
    }
}
