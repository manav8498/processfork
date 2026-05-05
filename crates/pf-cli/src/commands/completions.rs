// SPDX-License-Identifier: MIT
//! `pf completions` — emit shell-completion scripts.

use clap::CommandFactory;
use clap_complete::{Shell, generate};

#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Target shell.
    pub shell: Shell,
}

pub fn run(args: Args) -> anyhow::Result<()> {
    // We need the same Cli the binary uses to render the right completion.
    // Trick: re-derive the command by importing main's Cli via the
    // crate's bin lookup. Since `main` doesn't expose Cli, we re-build a
    // minimal command tree here that mirrors main.rs's `Cli`. Keeping it
    // separate is harmless because clap_complete only needs the structure.
    #[derive(clap::Parser)]
    #[command(name = "pf", version)]
    struct PfShim {
        #[command(subcommand)]
        _sub: PfShimSub,
    }
    #[derive(clap::Subcommand)]
    enum PfShimSub {
        Snapshot,
        Fork,
        Checkout,
        Merge,
        Push,
        Pull,
        Clone,
        Log,
        Diff,
        Status,
        Gc,
        Verify,
        Completions,
    }
    let mut cmd = PfShim::command();
    let bin = cmd.get_name().to_string();
    generate(args.shell, &mut cmd, bin, &mut std::io::stdout());
    Ok(())
}
