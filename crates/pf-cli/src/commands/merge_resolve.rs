// SPDX-License-Identifier: MIT
//! `pf merge-resolve` — drop a merged image's FS into a workdir so the
//! operator can edit conflict-markered files by hand, then run
//! `pf merge-finalize` to produce a clean image.
//!
//! Flow:
//! ```text
//!   pf merge A B                       → CID_M  (may have conflicts)
//!   pf merge-resolve  CID_M --workdir /tmp/x
//!   $EDITOR /tmp/x/path/with/markers   ← human-resolves
//!   pf merge-finalize CID_M --workdir /tmp/x  → CID_F (clean)
//! ```
//!
//! v1.0.12 audit fix: closes the v1.0.11 README's "conflict-merge
//! resolution UI is v1.1" gap. The merge engine has been writing
//! Git-style markers since v1.0.0; this command turns that into an
//! operator-runnable resolution flow.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::store::PfStore;

use super::CliError;

#[derive(Debug, Parser)]
pub struct Args {
    /// CID of the conflicted merge image (the one returned by
    /// `pf merge A B` when it exited 3 with conflicts).
    pub cid: String,

    /// Working directory to drop the merged FS into. Must NOT
    /// already exist — refused otherwise to avoid clobbering an
    /// in-progress resolution. Pair with `pf merge-finalize`.
    #[arg(long)]
    pub workdir: PathBuf,
}

pub fn run(store_root: &Path, args: Args) -> anyhow::Result<()> {
    let store = PfStore::open(store_root)?;
    let blobs: Arc<dyn BlobStore> = store.blobs_arc();
    let cid =
        Digest256::parse(&args.cid).map_err(|e| CliError::BadInput(format!("bad CID: {e}")))?;

    if args.workdir.exists() {
        return Err(CliError::BadInput(format!(
            "--workdir already exists: {}. Refusing to clobber.",
            args.workdir.display()
        ))
        .into());
    }

    let manifest = store.get_manifest(&cid)?;
    pf_world::restore_tree(&blobs, &manifest.world.fs, &args.workdir)?;

    // Walk the freshly-restored tree and report any files containing
    // Git-style conflict markers. We scan post-checkout rather than
    // querying the engine because the merge engine doesn't persist
    // its conflicts list — the markers IN the FS blob are the
    // source of truth, so scanning them is honest.
    let conflicts = scan_conflict_markers(&args.workdir)?;

    println!(
        "Restored merge image {} into {}",
        short(&args.cid),
        args.workdir.display()
    );
    if conflicts.is_empty() {
        println!(
            "(no conflict markers found — image is already clean; run pf merge-finalize to produce a single-parent image)"
        );
    } else {
        println!();
        println!("{} file(s) need resolution:", conflicts.len());
        for path in &conflicts {
            println!("  {}", path.display());
        }
        println!();
        println!("Edit them by hand to resolve the <<<<<<< / ======= / >>>>>>> markers,");
        println!("then run:");
        println!();
        println!(
            "  pf merge-finalize {} --workdir {}",
            args.cid,
            args.workdir.display()
        );
    }
    Ok(())
}

/// Walk `root` and return paths whose contents contain any of the
/// Git-style conflict marker prefixes (`<<<<<<<`, `=======`,
/// `>>>>>>>`). Symlinks and binary files are skipped.
///
/// Public so `pf merge-finalize` can run the same scan to refuse
/// finalization until conflicts are resolved.
pub fn scan_conflict_markers(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                walk(&path, out)?;
                continue;
            }
            if !ty.is_file() {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            // Heuristic: skip files containing any NUL bytes (binary).
            if bytes.contains(&0u8) {
                continue;
            }
            // Ascii-needle scan; the merge engine writes markers with
            // exactly seven `<`, `=`, `>` chars at line start.
            let needles: [&[u8]; 3] = [b"\n<<<<<<<", b"\n=======", b"\n>>>>>>>"];
            let starts_at_zero: [&[u8]; 3] = [b"<<<<<<<", b"=======", b">>>>>>>"];
            let has_marker = bytes
                .windows(8)
                .any(|w| needles.iter().any(|n| w.starts_with(n)))
                || starts_at_zero.iter().any(|n| bytes.starts_with(n));
            if has_marker {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn short(cid: &str) -> String {
    if cid.len() > 16 {
        format!("{}…", &cid[..16])
    } else {
        cid.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_three_marker_styles() {
        let dir = tempfile::tempdir().unwrap();
        let conflict = dir.path().join("conflict.txt");
        std::fs::write(
            &conflict,
            "before\n<<<<<<< A\nlinea\n=======\nlineb\n>>>>>>> B\nafter\n",
        )
        .unwrap();
        let clean = dir.path().join("clean.txt");
        std::fs::write(&clean, "no markers here\n").unwrap();

        let found = scan_conflict_markers(dir.path()).unwrap();
        assert_eq!(found, vec![conflict]);
    }

    #[test]
    fn skips_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("a.bin");
        std::fs::write(&bin, b"\x00<<<<<<< A\n").unwrap();
        let found = scan_conflict_markers(dir.path()).unwrap();
        assert!(
            found.is_empty(),
            "binary files must not match: got {found:?}"
        );
    }
}
