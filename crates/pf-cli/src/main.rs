// SPDX-License-Identifier: MIT
//! `pf` — ProcessFork command-line interface.
//!
//! Phase 8: every subcommand from `agent_docs/cli-spec.md` is wired
//! to the layer crates. Subcommands that depend on Phase 9 (registry)
//! — `push`, `pull`, `clone` — currently exit 2 with a clear message
//! pointing at the deferred work.

#![forbid(unsafe_code)]

mod commands;

use std::path::PathBuf;

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

    /// CAS / index location. Defaults to `$PF_STORE` then `~/.processfork`.
    #[arg(long, global = true, env = "PF_STORE")]
    store: Option<PathBuf>,

    /// Disable ANSI colour. Honours `NO_COLOR` automatically.
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Capture a sandbox + chat trace into a `.pfimg`.
    Snapshot(commands::snapshot::Args),
    /// Branch one or more divergent live agents from a snapshot.
    Fork(commands::fork::Args),
    /// Restore the world-layer FS tree of an image.
    Checkout(commands::checkout::Args),
    /// Three-way merge B into A.
    Merge(commands::merge::Args),
    /// Push an image to a registry (`hf://…`, `s3://…`, `ipfs://…`, `oci://…`).
    Push(commands::stub::PushArgs),
    /// Pull an image from a registry.
    Pull(commands::stub::PullArgs),
    /// Pull and immediately check out an image.
    Clone(commands::stub::CloneArgs),
    /// Show the snapshot DAG.
    Log(commands::log::Args),
    /// Diff two images across all four layers.
    Diff(commands::diff::Args),
    /// Show local store status.
    Status,
    /// Garbage-collect unreferenced blobs.
    Gc(commands::gc::Args),
    /// Verify the integrity of every blob in the local store.
    Verify(commands::verify::Args),
    /// Generate shell completions.
    Completions(commands::completions::Args),
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    let store_root = resolve_store_root(cli.store);

    let result: anyhow::Result<()> = match cli.command {
        Command::Snapshot(a) => commands::snapshot::run(&store_root, a),
        Command::Fork(a) => commands::fork::run(&store_root, a),
        Command::Checkout(a) => commands::checkout::run(&store_root, a),
        Command::Merge(a) => commands::merge::run(&store_root, a),
        Command::Log(a) => commands::log::run(&store_root, a),
        Command::Diff(a) => commands::diff::run(&store_root, a),
        Command::Status => commands::status::run(&store_root),
        Command::Gc(a) => commands::gc::run(&store_root, a),
        Command::Verify(a) => commands::verify::run(&store_root, a),
        Command::Completions(a) => commands::completions::run(a),
        Command::Push(a) => commands::stub::push(a),
        Command::Pull(a) => commands::stub::pull(a),
        Command::Clone(a) => commands::stub::clone(a),
    };

    match result {
        Ok(()) => std::process::ExitCode::from(0),
        Err(e) => {
            eprintln!("pf: error: {e:#}");
            // Exit code 3 for merge conflict, 2 for not-yet-implemented,
            // 4 for integrity, 1 for everything else (matches cli-spec.md).
            let code: u8 = e
                .downcast_ref::<commands::CliError>()
                .map_or(1, commands::CliError::exit_code);
            std::process::ExitCode::from(code)
        }
    }
}

fn resolve_store_root(arg: Option<PathBuf>) -> PathBuf {
    if let Some(p) = arg {
        return p;
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".processfork");
    }
    PathBuf::from(".processfork")
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = match verbosity {
        0 => EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        1 => EnvFilter::new("info"),
        2 => EnvFilter::new("debug"),
        _ => EnvFilter::new("trace"),
    };
    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();
}
