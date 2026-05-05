// SPDX-License-Identifier: MIT
//! `pf status` — show local-store summary.

use std::path::Path;

use pf_core::store::PfStore;

pub fn run(store_root: &Path) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let bytes = store.blobs().physical_bytes()?;
    let manifests: Vec<_> = store.iter_manifests()?.collect();
    println!("store      : {}", store.root().display());
    println!("manifests  : {}", manifests.len());
    println!(
        "blobs size : {} bytes ({:.2} MiB)",
        bytes,
        bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}
