// SPDX-License-Identifier: MIT
//! `pf` — ProcessFork command-line interface.
//!
//! Phase 0 scaffold: every subcommand parses correctly and prints a
//! "not yet implemented" message. Real wiring to the layer crates lands in
//! Phase 8.

#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "pf",
    version,
    about = "ProcessFork — fork() for AI agents",
    long_about = None,
    propagate_version = true,
)]
struct Cli {
    /// Verbose tracing output (`-v`, `-vv`, `-vvv`).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Capture a live agent into a `.pfimg`.
    Snapshot { agent_id: String },
    /// Branch one or more divergent live agents from a snapshot.
    Fork {
        /// Source content-id.
        cid: String,
        /// Number of branches to spawn.
        #[arg(short = 'n', long, default_value_t = 1)]
        count: u32,
        /// Optional steering hint.
        #[arg(long)]
        explore: Option<String>,
    },
    /// Restore a snapshot on this machine.
    Checkout { cid: String },
    /// Three-way merge a branch back into another.
    Merge {
        from: String,
        #[arg(long)]
        into: String,
    },
    /// Push an image to a registry (`hf://…`, `s3://…`, `ipfs://…`).
    Push { image: String, target: String },
    /// Pull an image from a registry into the local store.
    Pull { source: String },
    /// Pull and immediately check out an image.
    Clone { source: String },
    /// Show the snapshot graph for an agent or store.
    Log,
    /// Diff two images across all four layers.
    Diff { a: String, b: String },
    /// Show the local store status.
    Status,
    /// Garbage-collect unreferenced blobs.
    Gc,
    /// Verify the integrity of all blobs in the local store.
    Verify,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        Command::Snapshot { .. }
        | Command::Fork { .. }
        | Command::Checkout { .. }
        | Command::Merge { .. }
        | Command::Push { .. }
        | Command::Pull { .. }
        | Command::Clone { .. }
        | Command::Log
        | Command::Diff { .. }
        | Command::Status
        | Command::Gc
        | Command::Verify => {
            eprintln!(
                "pf: scaffold only — this subcommand is not yet wired up. \
                 See claude-progress.json for implementation status."
            );
            std::process::exit(2);
        }
    }
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = match verbosity {
        0 => EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        1 => EnvFilter::new("info"),
        2 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    };
    fmt().with_env_filter(filter).with_target(true).init();
}
