// SPDX-License-Identifier: MIT
//! Subcommands deferred to Phase 9 (registry).

use clap::Parser;

use super::CliError;

#[derive(Debug, Parser)]
pub struct PushArgs {
    /// Image to push.
    pub cid: String,
    /// Target URL — `hf://user/repo`, `s3://bucket/prefix`, `oci://…`, `ipfs://CID`.
    pub target: String,
}

#[derive(Debug, Parser)]
pub struct PullArgs {
    /// Source URL — same scheme set as `push`.
    pub source: String,
}

#[derive(Debug, Parser)]
pub struct CloneArgs {
    /// Source URL — same scheme set as `push`.
    pub source: String,
    /// Where to materialise the world-layer FS tree.
    #[arg(long)]
    pub into: Option<std::path::PathBuf>,
}

pub fn push(_a: PushArgs) -> anyhow::Result<()> {
    Err(CliError::NotYetImplemented(
        "`pf push` lands in Phase 9 (registry adapters). The CLI scaffold and \
         wire format are stable; the HF / S3 / IPFS / OCI implementations are \
         tracked in claude-progress.json under phase 9."
            .into(),
    )
    .into())
}

pub fn pull(_a: PullArgs) -> anyhow::Result<()> {
    Err(CliError::NotYetImplemented("`pf pull` lands in Phase 9 (registry).".into()).into())
}

pub fn clone(_a: CloneArgs) -> anyhow::Result<()> {
    Err(CliError::NotYetImplemented(
        "`pf clone` (= pull + checkout) lands in Phase 9 (registry).".into(),
    )
    .into())
}
