// SPDX-License-Identifier: MIT
//! `push` / `pull` / `clone` — registry adapters wired in Phase 9.
//!
//! For schemes the build doesn't support (HF / S3 / IPFS without their
//! `*-live` features), the underlying [`pf_registry::Registry`] returns
//! `RegistryError::UnsupportedScheme`; we forward that as exit code 2.
//!
//! The module is named `stub` for historical reasons (it held the
//! Phase-8 stubs); v1.1 will rename to `commands::registry`.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::store::PfStore;
use pf_registry::{ImageRef, RegistryError};

use super::CliError;

#[derive(Debug, Parser)]
pub struct PushArgs {
    /// Image to push.
    pub cid: String,
    /// Target URL — `file:///abs/path`, `hf://user/repo`,
    /// `s3://bucket/prefix`, `oci://host[:port]/repo`, `ipfs://CID`.
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
    pub into: Option<PathBuf>,
}

fn map_reg_err(e: RegistryError) -> anyhow::Error {
    match e {
        RegistryError::UnsupportedScheme(s) => CliError::NotYetImplemented(s).into(),
        RegistryError::BadRef(e) => CliError::BadInput(format!("ref: {e}")).into(),
        RegistryError::SignatureVerify(s) => CliError::Integrity(format!("signature: {s}")).into(),
        RegistryError::Core(e) => anyhow::anyhow!("core: {e}"),
        RegistryError::Backend(s) => anyhow::anyhow!("backend: {s}"),
    }
}

fn auth_from_env() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for k in [
        "HF_TOKEN",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_REGION",
        "AWS_ENDPOINT_URL",
        "IPFS_API",
    ] {
        if let Ok(v) = std::env::var(k) {
            m.insert(k.to_owned(), v);
        }
    }
    m
}

pub fn push(store_root: &Path, args: PushArgs) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let cid =
        Digest256::parse(&args.cid).map_err(|e| CliError::BadInput(format!("bad cid: {e}")))?;
    let manifest = store.get_manifest(&cid)?;
    let target = ImageRef::parse(&args.target).map_err(|e| map_reg_err(e.into()))?;
    let auth = auth_from_env();
    let reg = pf_registry::open(&target, &auth).map_err(map_reg_err)?;
    let blobs = store.blobs_arc();
    block_on(reg.push(&target, &manifest, blobs.as_ref())).map_err(map_reg_err)?;
    println!("✓ pushed {cid} → {}", args.target);
    Ok(())
}

pub fn pull(store_root: &Path, args: PullArgs) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let source = ImageRef::parse(&args.source).map_err(|e| map_reg_err(e.into()))?;
    let auth = auth_from_env();
    let reg = pf_registry::open(&source, &auth).map_err(map_reg_err)?;
    let layer_set = block_on(reg.pull(&source)).map_err(map_reg_err)?;
    let blobs = store.blobs_arc();
    for (digest, bytes) in &layer_set.blobs {
        let put_d = blobs.put(bytes)?;
        if &put_d != digest {
            return Err(CliError::Integrity(format!(
                "pulled blob bytes hash to {put_d}, registry recorded {digest}"
            ))
            .into());
        }
    }
    let cid = store.put_manifest(&layer_set.manifest)?;
    println!("{cid}");
    Ok(())
}

pub fn clone(store_root: &Path, args: CloneArgs) -> anyhow::Result<()> {
    // 1. Pull (writes blobs + manifest into the local store).
    let store = PfStore::open(store_root)?;
    let source = ImageRef::parse(&args.source).map_err(|e| map_reg_err(e.into()))?;
    let auth = auth_from_env();
    let reg = pf_registry::open(&source, &auth).map_err(map_reg_err)?;
    let layer_set = block_on(reg.pull(&source)).map_err(map_reg_err)?;
    let blobs: Arc<dyn BlobStore> = store.blobs_arc();
    for (digest, bytes) in &layer_set.blobs {
        let put_d = blobs.put(bytes)?;
        if &put_d != digest {
            return Err(CliError::Integrity(format!(
                "pulled blob bytes hash to {put_d}, registry recorded {digest}"
            ))
            .into());
        }
    }
    let cid = store.put_manifest(&layer_set.manifest)?;
    println!("{cid}");

    // 2. Optionally check out the world-layer FS tree.
    if let Some(into) = args.into {
        pf_world::restore_tree(&blobs, &layer_set.manifest.world.fs, &into)?;
        println!("✓ restored to {}", into.display());
    }
    Ok(())
}

/// Single-shot tokio runtime for the small async surface of pf-registry.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(f)
}
