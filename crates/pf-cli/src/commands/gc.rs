// SPDX-License-Identifier: MIT
//! `pf gc` — mark-and-sweep over orphaned blobs.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use clap::Parser;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::Manifest;
use pf_core::store::PfStore;

#[derive(Debug, Parser)]
pub struct Args {
    /// Retain manifests with the most recent N `created_at` (oldest GC'd
    /// first). 0 means keep every reachable manifest.
    #[arg(long, default_value_t = 0)]
    pub retain_recent: usize,
    /// Print what would be deleted without touching disk.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;

    // Materialize the full manifest universe BEFORE truncation —
    // we need the list of evicted CIDs (those we're NOT keeping) so
    // we can also delete their `images/<cid>.json` markers. The
    // v1.0.12 retest reproduced "pf gc --retain-recent N leaves
    // dangling log entries": GC was deleting the layer blobs of
    // older manifests but never the marker files that `pf log`
    // walks, so `pf log` happily listed CIDs whose `pf checkout`
    // would fail. v1.0.13 fix.
    let mut all_manifests: Vec<(Digest256, Manifest)> = store.iter_manifests()?.collect();
    all_manifests.sort_by_key(|(_, m)| std::cmp::Reverse(m.created_at));

    let kept: Vec<(Digest256, Manifest)> =
        if args.retain_recent > 0 && all_manifests.len() > args.retain_recent {
            all_manifests
                .iter()
                .take(args.retain_recent)
                .cloned()
                .collect()
        } else {
            all_manifests.clone()
        };

    // The set of manifest CIDs we are evicting — used for marker
    // pruning AFTER the blob sweep.
    let kept_cids: HashSet<Digest256> = kept.iter().map(|(cid, _)| cid.clone()).collect();
    let evicted_cids: Vec<Digest256> = all_manifests
        .iter()
        .filter(|(cid, _)| !kept_cids.contains(cid))
        .map(|(cid, _)| cid.clone())
        .collect();

    let blobs: Arc<dyn BlobStore> = store.blobs_arc();
    let mut reachable: HashSet<Digest256> = HashSet::new();
    for (cid, m) in &kept {
        reachable.insert(cid.clone());
        gather_layer_digests(m, &mut reachable);
        // Plus the JSON of the manifest itself.
        let bytes = serde_json::to_vec(m)?;
        reachable.insert(Digest256::of(&bytes));
        // v1.0.3 audit fix: walk the FsTree's per-file blobs and the
        // PageManifest's per-page K/V blobs so retain_recent doesn't
        // delete them out from under a retained manifest.
        if let Ok(transitive) = pf_registry::registry::transitive_blob_digests(m, blobs.as_ref()) {
            for d in transitive {
                reachable.insert(d);
            }
        }
    }

    // Walk every on-disk blob; delete the unreachable ones.
    let blobs_dir = store.root().join("blobs").join("sha256");
    let mut unreachable_count: u64 = 0;
    let mut bytes_freed: u64 = 0;
    if blobs_dir.exists() {
        for shard in std::fs::read_dir(&blobs_dir)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for blob in std::fs::read_dir(shard.path())? {
                let blob = blob?;
                let name = blob.file_name().to_string_lossy().to_string();
                let hex = name.strip_suffix(".zst").unwrap_or(&name);
                let Ok(d) = Digest256::parse(&format!("sha256:{hex}")) else {
                    continue;
                };
                if !reachable.contains(&d) {
                    let size = blob.metadata().map_or(0, |m| m.len());
                    unreachable_count += 1;
                    bytes_freed += size;
                    if !args.dry_run {
                        let _ = std::fs::remove_file(blob.path());
                    }
                }
            }
        }
    }

    // v1.0.13 fix: prune the `images/<cid>.json` markers for every
    // evicted manifest so `pf log` no longer lists CIDs whose
    // layer blobs are gone. Without this, retain_recent is a
    // referential-integrity bug — the index says "this CID
    // exists" while the CAS says "I have no idea what you're
    // talking about".
    let images_dir = store.root().join("images");
    let mut marker_count: u64 = 0;
    if images_dir.exists() {
        for cid in &evicted_cids {
            let marker = images_dir.join(format!("{}.json", cid.hex()));
            if marker.exists() {
                marker_count += 1;
                if !args.dry_run {
                    let _ = std::fs::remove_file(&marker);
                }
            }
        }
    }

    println!(
        "{} {} unreachable blobs ({} bytes) and {} stale image markers",
        if args.dry_run {
            "would delete"
        } else {
            "deleted"
        },
        unreachable_count,
        bytes_freed,
        marker_count,
    );
    Ok(())
}

fn gather_layer_digests(m: &Manifest, out: &mut HashSet<Digest256>) {
    out.insert(m.model.base.clone());
    out.insert(m.model.diff.clone());
    out.insert(m.cache.manifest.clone());
    out.insert(m.world.fs.clone());
    out.insert(m.world.env.clone());
    out.insert(m.world.procs.clone());
    out.insert(m.effects.ledger.clone());
    out.insert(m.trace.messages.clone());
    // The transitive walk (FsTree-nested file blobs + PageManifest
    // K/V page blobs) lives in the caller via
    // pf_registry::registry::transitive_blob_digests so this function
    // can stay sync + side-effect-free.
}
