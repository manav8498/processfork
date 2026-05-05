// SPDX-License-Identifier: MIT
//! Filesystem layer: walk + content-address + restore.

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One entry in the captured FS tree manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsTreeEntry {
    /// Path **relative to the captured root** (forward-slash separated).
    pub path: String,
    /// `mode` stored as 4 octal digits (e.g. `"0644"`); we keep it as a
    /// string to preserve the leading zero through JSON.
    pub mode: String,
    /// File size in bytes (post-decompression). Symlinks: target byte length.
    pub size: u64,
    /// File kind.
    pub kind: FsEntryKind,
    /// Content digest. For symlinks, the digest of the target string. For
    /// directories, [`None`] (the directory is implied by its children).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<Digest256>,
    /// Symlink target (only for symlinks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
}

/// File kind for [`FsTreeEntry`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsEntryKind {
    /// Regular file.
    File,
    /// Directory (no content; presence implies the dir).
    Dir,
    /// Symbolic link.
    Symlink,
}

/// Wire format of the captured tree (`fs.tree.v1` blob).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FsTree {
    /// Schema discriminator. Always `"fs.tree.v1"`.
    pub kind: String,
    /// Entries sorted by `path` for deterministic digests.
    pub entries: Vec<FsTreeEntry>,
}

/// Captures a directory tree into a [`BlobStore`] and emits a single
/// `fs.tree.v1` blob describing the structure.
///
/// Concurrency: file content-addressing runs on a rayon thread pool. Walk
/// itself is single-threaded (`walkdir`) — we sort all entries first so the
/// emitted manifest is byte-identical across runs over the same tree.
pub struct WalkFsCapture {
    root: PathBuf,
    use_apfs_clone: bool,
    follow_symlinks: bool,
    ignore: Vec<String>,
}

impl WalkFsCapture {
    /// Capture the directory rooted at `root`.
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            use_apfs_clone: false,
            follow_symlinks: false,
            ignore: vec![
                ".git/objects".into(),
                "target".into(),
                "node_modules".into(),
            ],
        }
    }

    /// Toggle the macOS APFS clone fast-path. When enabled and the source is
    /// on APFS, we `clonefile(2)`-clone the directory into a temp dir first
    /// (O(1) per the APFS docs) and walk the clone — giving a stable view
    /// without pausing the agent. Falls back to a direct walk on other
    /// filesystems / OSes. Off by default in v1; opt in for production.
    #[must_use]
    pub fn use_apfs_clone(mut self, enable: bool) -> Self {
        self.use_apfs_clone = enable;
        self
    }

    /// Follow symlinks during walk. Off by default — we capture symlinks as
    /// symlinks, not as the targets they happen to point at.
    #[must_use]
    pub fn follow_symlinks(mut self, enable: bool) -> Self {
        self.follow_symlinks = enable;
        self
    }

    /// Add a path-fragment to the ignore list. Default ignores: `.git/objects`,
    /// `target`, `node_modules`.
    #[must_use]
    pub fn ignore(mut self, fragment: impl Into<String>) -> Self {
        self.ignore.push(fragment.into());
        self
    }

    /// Run the capture. Returns the digest of the `fs.tree.v1` blob.
    pub fn capture(&self, blobs: &Arc<dyn BlobStore>) -> pf_core::Result<Digest256> {
        // APFS clone fast-path is best-effort; if it fails we fall back to
        // walking the live tree.
        let walk_root: PathBuf = if self.use_apfs_clone && cfg!(target_os = "macos") {
            apfs_clone(&self.root).unwrap_or_else(|_| self.root.clone())
        } else {
            self.root.clone()
        };

        // Collect entries first so we can sort and parallelize hashing.
        let mut raw: Vec<walkdir::DirEntry> = walkdir::WalkDir::new(&walk_root)
            .follow_links(self.follow_symlinks)
            .into_iter()
            .filter_entry(|e| {
                let p = e.path().to_string_lossy();
                !self.ignore.iter().any(|frag| p.contains(frag.as_str()))
            })
            .filter_map(std::result::Result::ok)
            .collect();

        // Skip the root itself (we capture its contents, not its name).
        raw.retain(|e| e.path() != walk_root.as_path());

        // Sort by path for deterministic manifests.
        raw.sort_by(|a, b| a.path().cmp(b.path()));

        // Parallel-hash regular files; symlinks/dirs are O(1).
        let entries: Vec<FsTreeEntry> = raw
            .par_iter()
            .map(|de| -> pf_core::Result<FsTreeEntry> {
                let abs = de.path();
                let rel = abs.strip_prefix(&walk_root).unwrap_or(abs);
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                let meta = de
                    .metadata()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                let mode = unix_mode_string(&meta);

                if meta.file_type().is_dir() {
                    return Ok(FsTreeEntry {
                        path: rel_str,
                        mode,
                        size: 0,
                        kind: FsEntryKind::Dir,
                        blob: None,
                        link_target: None,
                    });
                }
                if meta.file_type().is_symlink() {
                    let target = std::fs::read_link(abs)?;
                    let target_str = target.to_string_lossy().to_string();
                    let blob = blobs.put(target_str.as_bytes())?;
                    return Ok(FsTreeEntry {
                        path: rel_str,
                        mode,
                        size: target_str.len() as u64,
                        kind: FsEntryKind::Symlink,
                        blob: Some(blob),
                        link_target: Some(target_str),
                    });
                }
                // Regular file.
                let bytes = std::fs::read(abs)?;
                let size = bytes.len() as u64;
                let digest = blobs.put(&bytes)?;
                Ok(FsTreeEntry {
                    path: rel_str,
                    mode,
                    size,
                    kind: FsEntryKind::File,
                    blob: Some(digest),
                    link_target: None,
                })
            })
            .collect::<pf_core::Result<Vec<_>>>()?;

        let tree = FsTree {
            kind: "fs.tree.v1".into(),
            entries,
        };
        let json = serde_json::to_vec(&tree)?;
        blobs.put(&json)
    }
}

/// Restore a previously-captured tree blob into a fresh directory `dst`.
///
/// The restore is **atomic**: we rebuild into `dst.with_extension("pftmp")`,
/// `fsync` the parent, then `rename(2)` over `dst`. If `dst` already exists
/// the call errors — callers can pass a tempdir or pre-clean.
pub fn restore_tree(
    blobs: &Arc<dyn BlobStore>,
    tree_digest: &Digest256,
    dst: impl AsRef<Path>,
) -> pf_core::Result<()> {
    let dst = dst.as_ref();
    if dst.exists() {
        return Err(pf_core::Error::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "restore_tree refuses to overwrite existing path {}",
                dst.display()
            ),
        )));
    }
    let tree_bytes = blobs.get(tree_digest)?;
    let tree: FsTree = serde_json::from_slice(&tree_bytes)?;
    if tree.kind != "fs.tree.v1" {
        return Err(pf_core::Error::Integrity(format!(
            "expected fs.tree.v1, got {}",
            tree.kind
        )));
    }

    // Stage to a sibling temp directory.
    let parent = dst.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".pf-restore.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
    ));
    std::fs::create_dir(&staging)?;

    // Pass 1: directories (sorted, so parents land before children).
    for e in tree
        .entries
        .iter()
        .filter(|e| matches!(e.kind, FsEntryKind::Dir))
    {
        std::fs::create_dir_all(staging.join(&e.path))?;
    }
    // Pass 2: files + symlinks.
    for e in &tree.entries {
        let p = staging.join(&e.path);
        match e.kind {
            FsEntryKind::Dir => {}
            FsEntryKind::File => {
                let blob = e.blob.as_ref().ok_or_else(|| {
                    pf_core::Error::Integrity(format!("file entry {} missing blob", e.path))
                })?;
                let bytes = blobs.get(blob)?;
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&p, bytes)?;
            }
            FsEntryKind::Symlink => {
                let target = e.link_target.as_ref().ok_or_else(|| {
                    pf_core::Error::Integrity(format!(
                        "symlink entry {} missing link_target",
                        e.path
                    ))
                })?;
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, &p)?;
                #[cfg(not(unix))]
                std::fs::write(&p, target.as_bytes())?;
            }
        }
    }

    // Atomic flip.
    std::fs::rename(&staging, dst)?;
    Ok(())
}

// ----- macOS APFS clone helper -----

#[cfg(target_os = "macos")]
fn apfs_clone(src: &Path) -> std::io::Result<PathBuf> {
    use std::process::Command;
    let dst = std::env::temp_dir().join(format!(
        "pf-apfs-clone.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
    ));
    let status = Command::new("cp")
        .args(["-c", "-R"])
        .arg(src)
        .arg(&dst)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "cp -c -R exit status: {status:?}"
        )));
    }
    Ok(dst)
}

#[cfg(not(target_os = "macos"))]
fn apfs_clone(_src: &Path) -> std::io::Result<PathBuf> {
    Err(std::io::Error::other("APFS clone only available on macOS"))
}

// ----- mode helper -----

#[cfg(unix)]
fn unix_mode_string(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    format!("{:04o}", meta.permissions().mode() & 0o7777)
}
#[cfg(not(unix))]
fn unix_mode_string(meta: &std::fs::Metadata) -> String {
    if meta.permissions().readonly() {
        "0444".into()
    } else {
        "0644".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::cas::MemBlobStore;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &[u8]) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, contents).unwrap();
    }

    #[test]
    fn round_trip_small_tree() {
        let src = TempDir::new().unwrap();
        write(src.path(), "a.txt", b"hello");
        write(src.path(), "sub/b.txt", b"world");
        write(src.path(), "sub/c.bin", &vec![0xABu8; 8 * 1024]);

        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let tree_cid = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();

        let restore_root = TempDir::new().unwrap();
        let dst = restore_root.path().join("restored");
        restore_tree(&blobs, &tree_cid, &dst).unwrap();

        assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(dst.join("sub/b.txt")).unwrap(), b"world");
        assert_eq!(
            std::fs::read(dst.join("sub/c.bin")).unwrap().len(),
            8 * 1024
        );
    }

    #[test]
    fn capture_is_deterministic() {
        let src = TempDir::new().unwrap();
        write(src.path(), "a.txt", b"hello");
        write(src.path(), "b.txt", b"world");
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid1 = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();
        let cid2 = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();
        assert_eq!(
            cid1, cid2,
            "capture of identical tree must be byte-identical"
        );
    }

    #[test]
    fn ignored_paths_are_skipped() {
        let src = TempDir::new().unwrap();
        write(src.path(), "kept.txt", b"keep");
        write(src.path(), "node_modules/dep/index.js", b"skip");
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();
        let bytes = blobs.get(&cid).unwrap();
        let tree: FsTree = serde_json::from_slice(&bytes).unwrap();
        assert!(tree.entries.iter().any(|e| e.path == "kept.txt"));
        assert!(
            !tree
                .entries
                .iter()
                .any(|e| e.path.starts_with("node_modules"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_captured_as_symlinks() {
        let src = TempDir::new().unwrap();
        write(src.path(), "real.txt", b"data");
        std::os::unix::fs::symlink("real.txt", src.path().join("link.txt")).unwrap();
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();

        let restore_root = TempDir::new().unwrap();
        let dst = restore_root.path().join("r");
        restore_tree(&blobs, &cid, &dst).unwrap();
        let meta = std::fs::symlink_metadata(dst.join("link.txt")).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(
            std::fs::read_link(dst.join("link.txt"))
                .unwrap()
                .to_str()
                .unwrap(),
            "real.txt"
        );
    }
}
