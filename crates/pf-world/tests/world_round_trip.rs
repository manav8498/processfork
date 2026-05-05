// SPDX-License-Identifier: MIT
//! Phase-2 acceptance: build a synthetic sandbox, capture all three world
//! sub-layers (FS / env / procs), restore the FS, byte-compare every file.
//!
//! Default size is 32 MiB across 256 files — runs in ~1 s on the build host.
//! Set `PF_WORLD_TEST_GB=1` to bump to a real 1 GB sandbox (the spec's
//! Phase-2 gate); CI runs the small variant by default and the gigabyte
//! variant on a nightly job.

// The few cast and Debug-format lints below are diagnostic; this is a test
// file and the readability win of `{path:?}` outweighs the pedantic clippy.
#![allow(clippy::cast_possible_truncation, clippy::unnecessary_debug_formatting)]

use std::sync::Arc;

use pf_core::cas::{BlobStore, FsBlobStore};
use pf_world::{EnvCapture, ProcsCapture, WalkFsCapture, restore_tree};
use tempfile::TempDir;

fn target_size_bytes() -> u64 {
    if std::env::var("PF_WORLD_TEST_GB").as_deref() == Ok("1") {
        1024 * 1024 * 1024
    } else {
        32 * 1024 * 1024
    }
}

fn populate_sandbox(root: &std::path::Path, total_bytes: u64) -> std::io::Result<()> {
    // Spread across 256 files, each ~total/256.
    let n_files: u64 = 256;
    let per_file = (total_bytes / n_files).max(1024);
    let mut buf = vec![0u8; per_file as usize];
    for ix in 0..n_files {
        // Cheap deterministic fill — just XORs in the file index every 8 B.
        for (chunk_ix, chunk) in buf.chunks_mut(8).enumerate() {
            let v = ix.wrapping_mul(0x9E37_79B9).wrapping_add(chunk_ix as u64);
            chunk.copy_from_slice(&v.to_le_bytes()[..chunk.len()]);
        }
        let sub = root.join(format!("d{:02x}", ix % 16));
        std::fs::create_dir_all(&sub)?;
        std::fs::write(sub.join(format!("file_{ix:04}.bin")), &buf)?;
    }
    // Throw in a symlink so we exercise that path.
    #[cfg(unix)]
    std::os::unix::fs::symlink("d00/file_0000.bin", root.join("alias.bin"))?;
    Ok(())
}

fn assert_trees_byte_equal(a: &std::path::Path, b: &std::path::Path) {
    use walkdir::WalkDir;
    let mut a_files: Vec<_> = WalkDir::new(a)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file() || e.file_type().is_symlink())
        .map(|e| {
            let rel = e.path().strip_prefix(a).unwrap().to_path_buf();
            (rel, e.path().to_path_buf())
        })
        .collect();
    a_files.sort();
    for (rel, abs) in &a_files {
        let other = b.join(rel);
        assert!(other.exists(), "{rel:?} missing from restored tree");
        let meta_a = std::fs::symlink_metadata(abs).unwrap();
        let meta_b = std::fs::symlink_metadata(&other).unwrap();
        assert_eq!(
            meta_a.file_type().is_symlink(),
            meta_b.file_type().is_symlink(),
            "{rel:?} symlink-ness diverged"
        );
        if meta_a.file_type().is_symlink() {
            assert_eq!(
                std::fs::read_link(abs).unwrap(),
                std::fs::read_link(&other).unwrap(),
                "{rel:?} symlink target diverged"
            );
        } else {
            assert_eq!(
                std::fs::read(abs).unwrap(),
                std::fs::read(&other).unwrap(),
                "{rel:?} content diverged"
            );
        }
    }
}

#[test]
fn fs_round_trip_byte_identical_on_sandbox() {
    let store_dir = TempDir::new().unwrap();
    let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(store_dir.path()).unwrap());

    let sandbox = TempDir::new().unwrap();
    let target = target_size_bytes();
    populate_sandbox(sandbox.path(), target).unwrap();

    let cid = WalkFsCapture::new(sandbox.path()).capture(&blobs).unwrap();

    let restored_root = TempDir::new().unwrap();
    let dst = restored_root.path().join("restored");
    restore_tree(&blobs, &cid, &dst).unwrap();

    assert_trees_byte_equal(sandbox.path(), &dst);
}

#[test]
fn env_capture_produces_deterministic_digest() {
    let store_dir = TempDir::new().unwrap();
    let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(store_dir.path()).unwrap());
    let c1 = EnvCapture::new()
        .scrub("(?i)secret|token|password")
        .unwrap()
        .capture(&blobs)
        .unwrap();
    let c2 = EnvCapture::new()
        .scrub("(?i)secret|token|password")
        .unwrap()
        .capture(&blobs)
        .unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn procs_capture_emits_a_blob_on_every_host() {
    let store_dir = TempDir::new().unwrap();
    let blobs: Arc<dyn BlobStore> = Arc::new(FsBlobStore::open(store_dir.path()).unwrap());
    let pid = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
    let cid = ProcsCapture::new([pid]).capture(&blobs).unwrap();
    let bytes = blobs.get(&cid).unwrap();
    assert!(!bytes.is_empty());
}
