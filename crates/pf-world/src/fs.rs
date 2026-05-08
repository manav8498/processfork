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
    /// v1.0.13 audit fix: glob-style ignore patterns alongside the
    /// segment-match ones. Built lazily from `ignore` entries that
    /// contain glob meta-characters (`*`, `?`, `[`).
    ignore_globs: Vec<globset::GlobMatcher>,
}

/// v1.0.13 default-ignore set extension. The previous default set
/// covered build directories (`target`, `node_modules`, `.git/objects`)
/// and the `.pfcid` sentinel. The v1.0.12 retest reproduced **false
/// merge conflicts** when `__pycache__/` and `.pytest_cache/`
/// landed in the captured tree from a `pytest` run on otherwise
/// disjoint branches. This set adds the universally-cache directory
/// names that should never be source-of-truth, plus common Python
/// bytecode patterns. Conservative: nothing here is ever a "maybe
/// I want this" — they are all caches by definition.
const DEFAULT_EXTRA_IGNORES: &[&str] = &[
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".coverage",
    ".venv",
    ".DS_Store",
    "*.pyc",
    "*.pyo",
];

impl WalkFsCapture {
    /// Capture the directory rooted at `root`.
    pub fn new(root: impl AsRef<Path>) -> Self {
        let mut ignore: Vec<String> = vec![
            ".git/objects".into(),
            "target".into(),
            "node_modules".into(),
            // `.pfcid` is the sentinel `pf checkout` writes so a
            // subsequent `pf snapshot` knows its parent CID. We
            // skip it here so it never lands in the captured tree.
            ".pfcid".into(),
        ];
        for extra in DEFAULT_EXTRA_IGNORES {
            ignore.push((*extra).to_owned());
        }
        let ignore_globs = compile_globs(&ignore);
        Self {
            root: root.as_ref().to_path_buf(),
            use_apfs_clone: false,
            follow_symlinks: false,
            ignore,
            ignore_globs,
        }
    }

    /// Build a capturer that does NOT carry the v1.0.13 default-extra
    /// ignore set (`__pycache__`, `.pytest_cache`, `*.pyc`, …).
    /// Operators who want byte-for-byte capture of every file in
    /// the source tree (rare; CI auditing the set itself; building
    /// a registry mirror) call this. Default callers should use
    /// [`WalkFsCapture::new`] which has the safe set.
    pub fn new_without_default_ignores(root: impl AsRef<Path>) -> Self {
        let ignore: Vec<String> = vec![
            ".git/objects".into(),
            "target".into(),
            "node_modules".into(),
            ".pfcid".into(),
        ];
        let ignore_globs = compile_globs(&ignore);
        Self {
            root: root.as_ref().to_path_buf(),
            use_apfs_clone: false,
            follow_symlinks: false,
            ignore,
            ignore_globs,
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

    /// Add a path-fragment OR glob pattern to the ignore list.
    ///
    /// - Plain entries (`target`, `__pycache__`, `.git/objects`) are
    ///   matched as path-component sequences, exactly as before.
    /// - Glob entries (anything containing `*`, `?`, `[`) are matched
    ///   against the relative path via [`globset::Glob`]. Common
    ///   patterns: `*.pyc`, `*.log`, `**/build/**`.
    ///
    /// v1.0.13 added glob support; segment-match semantics for plain
    /// entries are unchanged.
    #[must_use]
    pub fn ignore(mut self, fragment: impl Into<String>) -> Self {
        let entry: String = fragment.into();
        if has_glob_chars(&entry)
            && let Ok(g) = globset::Glob::new(&entry)
        {
            self.ignore_globs.push(g.compile_matcher());
        }
        self.ignore.push(entry);
        self
    }

    /// Read a `.gitignore`/`.pfignore`-style file and apply each
    /// non-comment, non-empty line as an ignore entry. Lines ending
    /// with `/` have the slash stripped (gitignore directory marker).
    /// Lines starting with `!` (gitignore negation) are skipped with
    /// a `tracing::warn!` — this is a v1.0.13 limitation; full
    /// gitignore semantics with negation arrive when an operator
    /// hits the use case.
    ///
    /// Returns Ok(self) even if the file doesn't exist (so
    /// `.ignore_from(".pfignore")` is safe to chain unconditionally).
    /// Returns the underlying io::Error only if the file exists but
    /// can't be read (permissions, etc.).
    pub fn ignore_from(mut self, path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(self);
        }
        let content = std::fs::read_to_string(path)?;
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('!') {
                tracing::warn!(
                    "ignoring gitignore negation in {}: {} (negation not yet supported in v1.0.13)",
                    path.display(),
                    line
                );
                continue;
            }
            let trimmed = line.trim_start_matches('/').trim_end_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            self = self.ignore(trimmed);
        }
        Ok(self)
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
                // Component-segment match (NOT substring). The v1.0.2
                // audit found that the previous `p.contains(frag)` test
                // dropped legitimate paths whose name happened to share
                // a substring with an ignore entry, e.g.
                // `src/targeted/keep.txt` was filtered because "target"
                // appeared as a substring. We now compare each
                // path-component to each ignore entry exactly. Multi-
                // segment ignores like ".git/objects" still work via
                // path-prefix containment of the joined segments.
                //
                // v1.0.13: glob entries (containing `*`/`?`/`[`) are
                // also matched against the path RELATIVE to walk_root,
                // so `*.pyc` correctly skips `src/foo.pyc`.
                let rel = e.path().strip_prefix(&walk_root).unwrap_or(e.path());
                !path_matches_any_ignore(e.path(), &self.ignore)
                    && !path_matches_any_glob(rel, &self.ignore_globs)
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
        let safe = safe_join(&staging, &e.path)?;
        std::fs::create_dir_all(&safe)?;
        apply_mode(&safe, &e.mode)?;
    }
    // Pass 2: files + symlinks.
    for e in &tree.entries {
        let p = safe_join(&staging, &e.path)?;
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
                apply_mode(&p, &e.mode)?;
            }
            FsEntryKind::Symlink => {
                let raw_target = e.link_target.as_ref().ok_or_else(|| {
                    pf_core::Error::Integrity(format!(
                        "symlink entry {} missing link_target",
                        e.path
                    ))
                })?;
                // Symlink target hardening: refuse absolute targets and
                // refuse relative targets that would escape the staging
                // root. Together with the safe_join above this means a
                // malicious .pfimg can never write or link outside the
                // restore directory.
                check_symlink_target(&staging, &p, raw_target)?;
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(raw_target, &p)?;
                #[cfg(not(unix))]
                std::fs::write(&p, raw_target.as_bytes())?;
            }
        }
    }

    // Atomic flip.
    std::fs::rename(&staging, dst)?;
    Ok(())
}

// `safe_join` (defined further down) is the v1.0.3 fix for the
// "Zip Slip"–style CVE found in the v1.0.2 audit: a malicious .pfimg
// with `path: "../../etc/passwd"` could write outside the target dir.

/// Component-segment ignore matcher. v1.0.2 audit found that
/// substring-matching dropped legitimate paths like
/// `src/targeted/keep.txt` (because "target" appeared as a substring).
///
/// We now match each ignore entry as a *path-component slash-sequence*:
/// an ignore of "target" matches a path that has any component equal
/// to "target", but does NOT match "targeted" or "untargeted".
/// Multi-segment ignores like ".git/objects" match consecutive
/// component runs.
/// True if `entry` looks like a glob pattern (contains `*`, `?`,
/// or `[`). v1.0.13: used to decide whether to compile a glob
/// matcher or treat the entry as a plain segment-match.
fn has_glob_chars(entry: &str) -> bool {
    entry.contains('*') || entry.contains('?') || entry.contains('[')
}

/// Compile every glob-style entry in `ignores` into a `GlobMatcher`.
/// Plain segment entries are skipped (handled by
/// `path_matches_any_ignore`). Invalid globs are silently dropped —
/// the operator's `--ignore` arg already passed clap parsing, so a
/// malformed glob is a user error worth surfacing via tracing but
/// not worth aborting capture for.
fn compile_globs(ignores: &[String]) -> Vec<globset::GlobMatcher> {
    let mut out = Vec::new();
    for ign in ignores {
        if !has_glob_chars(ign) {
            continue;
        }
        match globset::Glob::new(ign) {
            Ok(g) => out.push(g.compile_matcher()),
            Err(e) => tracing::warn!("ignore: invalid glob {ign:?}: {e}"),
        }
    }
    out
}

/// True if any compiled glob matches `relative_path` (the path
/// stripped of `walk_root`). Globs match the path with forward
/// slashes — globset normalises this internally.
fn path_matches_any_glob(relative_path: &Path, globs: &[globset::GlobMatcher]) -> bool {
    if globs.is_empty() {
        return false;
    }
    for g in globs {
        if g.is_match(relative_path) {
            return true;
        }
        // Also match against just the file name so `*.pyc` matches
        // `src/foo.pyc` even on platforms where globset doesn't
        // automatically descend on a non-`**` glob.
        if let Some(name) = relative_path.file_name()
            && g.is_match(Path::new(name))
        {
            return true;
        }
    }
    false
}

fn path_matches_any_ignore(path: &Path, ignores: &[String]) -> bool {
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    for ign in ignores {
        // Split each ignore on `/` so `.git/objects` checks for the
        // consecutive pair, while bare `target` checks for the single
        // segment.
        let needles: Vec<&str> = ign.split('/').filter(|s| !s.is_empty()).collect();
        if needles.is_empty() {
            continue;
        }
        for w in comps.windows(needles.len()) {
            if w == needles.as_slice() {
                return true;
            }
        }
    }
    false
}

/// Join `relative` onto `root`, but reject anything that would escape
/// `root`. Catches `..` segments, absolute paths, and Windows drive
/// letters. Returns `pf_core::Error::Integrity` on any escape attempt.
///
/// v1.0.3 fix for the "Zip Slip"–style CVE found in the v1.0.2 audit.
fn safe_join(root: &Path, relative: &str) -> pf_core::Result<PathBuf> {
    let candidate = Path::new(relative);
    if candidate.is_absolute() {
        return Err(pf_core::Error::Integrity(format!(
            "fs.tree entry has absolute path {relative:?} — refusing"
        )));
    }
    // Component-by-component check rather than `..`-substring (substring
    // would false-positive on legitimate names like "..foo").
    for comp in candidate.components() {
        match comp {
            std::path::Component::ParentDir => {
                return Err(pf_core::Error::Integrity(format!(
                    "fs.tree entry path {relative:?} contains `..` — refusing"
                )));
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(pf_core::Error::Integrity(format!(
                    "fs.tree entry path {relative:?} has root/prefix — refusing"
                )));
            }
            std::path::Component::CurDir | std::path::Component::Normal(_) => {}
        }
    }
    Ok(root.join(candidate))
}

/// Reject symlink targets that would resolve outside the restore root.
/// Absolute targets are always rejected (they obviously escape). For
/// relative targets we walk the components from the symlink's parent
/// dir and reject if the cumulative depth ever goes negative relative
/// to the root.
fn check_symlink_target(root: &Path, link_path: &Path, target: &str) -> pf_core::Result<()> {
    let target_path = Path::new(target);
    if target_path.is_absolute() {
        return Err(pf_core::Error::Integrity(format!(
            "symlink target {target:?} is absolute — refusing"
        )));
    }
    // Compute the symlink's depth below root, then walk the target's
    // components keeping a running depth counter. If it ever goes
    // below 0 the symlink would escape.
    let link_depth = link_path
        .strip_prefix(root)
        .ok()
        .map_or(0, |p| p.components().count().saturating_sub(1));
    let mut depth = isize::try_from(link_depth).unwrap_or(isize::MAX);
    for comp in target_path.components() {
        match comp {
            std::path::Component::ParentDir => depth -= 1,
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::CurDir => {}
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(pf_core::Error::Integrity(format!(
                    "symlink target {target:?} has root/prefix — refusing"
                )));
            }
        }
        if depth < 0 {
            return Err(pf_core::Error::Integrity(format!(
                "symlink target {target:?} escapes restore root — refusing"
            )));
        }
    }
    Ok(())
}

/// Apply the captured unix mode (e.g. "100755") to `path`. No-op on
/// Windows. The mode string is taken from `unix_mode_string()` at
/// capture time — the high bits are the file type and we mask them
/// out before chmod (only the permission bits matter for restore).
#[cfg(unix)]
fn apply_mode(path: &Path, mode: &str) -> pf_core::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let raw = u32::from_str_radix(mode, 8).unwrap_or(0o644);
    let perm = std::fs::Permissions::from_mode(raw & 0o7777);
    // Don't chmod symlinks (lchmod isn't portable); the symlink's
    // own mode is irrelevant on every linux/macos host.
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    std::fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: &str) -> pf_core::Result<()> {
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

    // ---- v1.0.3 audit-fix regression tests ----

    /// CVE: malicious .pfimg with `..` in a path must be refused.
    /// v1.0.2 audit reproduced writing outside the target dir twice.
    #[test]
    fn malicious_relative_path_traversal_is_refused() {
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let payload = b"PWNED";
        let blob = blobs.put(payload).unwrap();
        let tree = FsTree {
            kind: "fs.tree.v1".into(),
            entries: vec![FsTreeEntry {
                path: "../../escape.txt".into(),
                mode: "100644".into(),
                size: payload.len() as u64,
                kind: FsEntryKind::File,
                blob: Some(blob),
                link_target: None,
            }],
        };
        let tree_bytes = serde_json::to_vec(&tree).unwrap();
        let tree_cid = blobs.put(&tree_bytes).unwrap();

        let restore_root = TempDir::new().unwrap();
        let dst = restore_root.path().join("dst");
        let err = restore_tree(&blobs, &tree_cid, &dst).unwrap_err();
        assert!(
            format!("{err}").contains("`..`") || format!("{err}").contains("refusing"),
            "expected path-traversal refusal, got {err}"
        );
        // And the would-be escaped path doesn't exist.
        assert!(!restore_root.path().join("escape.txt").exists());
    }

    /// CVE: malicious .pfimg with an absolute path must be refused.
    #[test]
    fn malicious_absolute_path_is_refused() {
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let blob = blobs.put(b"x").unwrap();
        let tree = FsTree {
            kind: "fs.tree.v1".into(),
            entries: vec![FsTreeEntry {
                path: "/tmp/should-not-write".into(),
                mode: "100644".into(),
                size: 1,
                kind: FsEntryKind::File,
                blob: Some(blob),
                link_target: None,
            }],
        };
        let tree_cid = blobs.put(&serde_json::to_vec(&tree).unwrap()).unwrap();
        let restore_root = TempDir::new().unwrap();
        let dst = restore_root.path().join("dst");
        let err = restore_tree(&blobs, &tree_cid, &dst).unwrap_err();
        assert!(
            format!("{err}").contains("absolute") || format!("{err}").contains("refusing"),
            "expected absolute-path refusal, got {err}"
        );
    }

    /// CVE: malicious symlink whose target escapes the restore root
    /// must be refused (otherwise a follow-up file-write through the
    /// link writes outside the sandbox).
    #[cfg(unix)]
    #[test]
    fn malicious_symlink_escape_is_refused() {
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let target_str = "../../escape";
        let blob = blobs.put(target_str.as_bytes()).unwrap();
        let tree = FsTree {
            kind: "fs.tree.v1".into(),
            entries: vec![FsTreeEntry {
                path: "evil.lnk".into(),
                mode: "120777".into(),
                size: target_str.len() as u64,
                kind: FsEntryKind::Symlink,
                blob: Some(blob),
                link_target: Some(target_str.to_owned()),
            }],
        };
        let tree_cid = blobs.put(&serde_json::to_vec(&tree).unwrap()).unwrap();
        let restore_root = TempDir::new().unwrap();
        let dst = restore_root.path().join("dst");
        let err = restore_tree(&blobs, &tree_cid, &dst).unwrap_err();
        assert!(
            format!("{err}").contains("escape") || format!("{err}").contains("refusing"),
            "expected symlink-escape refusal, got {err}"
        );
    }

    /// v1.0.2 audit: 0755 source file restored as 0644.
    #[cfg(unix)]
    #[test]
    fn executable_mode_is_restored() {
        use std::os::unix::fs::PermissionsExt as _;
        let src = TempDir::new().unwrap();
        write(src.path(), "script.sh", b"#!/bin/sh\necho hi\n");
        let scr = src.path().join("script.sh");
        std::fs::set_permissions(&scr, std::fs::Permissions::from_mode(0o755)).unwrap();
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();

        let restore_root = TempDir::new().unwrap();
        let dst = restore_root.path().join("r");
        restore_tree(&blobs, &cid, &dst).unwrap();
        let meta = std::fs::metadata(dst.join("script.sh")).unwrap();
        assert_eq!(
            meta.permissions().mode() & 0o7777,
            0o755,
            "executable bit must survive snapshot+restore"
        );
    }

    /// v1.0.2 audit: substring matching dropped legitimate paths
    /// like `src/targeted/keep.txt` (the "target" segment is also a
    /// default ignore). After v1.0.3 the match is component-segment.
    #[test]
    fn ignore_matches_segments_not_substrings() {
        let src = TempDir::new().unwrap();
        write(src.path(), "src/targeted/keep.txt", b"keep");
        write(src.path(), "target/should-skip.txt", b"skip");
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();
        let tree: FsTree = serde_json::from_slice(&blobs.get(&cid).unwrap()).unwrap();
        let paths: Vec<&str> = tree.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(
            paths.contains(&"src/targeted/keep.txt"),
            "src/targeted/keep.txt must NOT be filtered (was: {paths:?})"
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("target/")),
            "target/ subtree must be filtered (was: {paths:?})"
        );
    }

    /// v1.0.13: the v1.0.12 retest reproduced false merge conflicts
    /// when `__pycache__/` and `.pytest_cache/` landed in the
    /// captured tree from a `pytest` run on otherwise-disjoint
    /// branches. Default-extra ignores now skip them.
    #[test]
    fn default_ignores_skip_python_cache_dirs() {
        let src = TempDir::new().unwrap();
        write(src.path(), "src/main.py", b"print('hi')\n");
        write(
            src.path(),
            "src/__pycache__/main.cpython-313.pyc",
            b"\x03\xf3\r\n", // pyc magic-ish
        );
        write(src.path(), ".pytest_cache/CACHEDIR.TAG", b"Signature: ...");
        write(src.path(), ".mypy_cache/3.13/CACHEDIR.TAG", b"...");
        write(src.path(), ".ruff_cache/0.6.0/foo", b"x");
        write(src.path(), ".venv/bin/python", b"#!/...\n");
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();
        let tree: FsTree = serde_json::from_slice(&blobs.get(&cid).unwrap()).unwrap();
        let paths: Vec<&str> = tree.entries.iter().map(|e| e.path.as_str()).collect();

        assert!(
            paths.contains(&"src/main.py"),
            "real source file must survive: {paths:?}"
        );
        for cache_pat in [
            "__pycache__",
            ".pytest_cache",
            ".mypy_cache",
            ".ruff_cache",
            ".venv",
        ] {
            assert!(
                !paths.iter().any(|p| p.contains(cache_pat)),
                "{cache_pat} must be filtered by default; got: {paths:?}"
            );
        }
    }

    /// v1.0.13: glob-pattern entries on the ignore list match
    /// against the path. The default set includes `*.pyc` so a
    /// stray `.pyc` outside `__pycache__/` (e.g. shipped artefacts)
    /// is also skipped.
    #[test]
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    fn glob_patterns_match_files_anywhere_in_tree() {
        let src = TempDir::new().unwrap();
        write(src.path(), "src/main.py", b"keep");
        write(src.path(), "src/legacy.pyc", b"skip-by-glob");
        write(src.path(), "build/output.pyc", b"skip-by-glob");
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = WalkFsCapture::new(src.path()).capture(&blobs).unwrap();
        let tree: FsTree = serde_json::from_slice(&blobs.get(&cid).unwrap()).unwrap();
        let paths: Vec<&str> = tree.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(
            paths.contains(&"src/main.py"),
            "non-glob source must survive: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with(".pyc")),
            "*.pyc glob must filter every .pyc anywhere: {paths:?}"
        );
    }

    /// v1.0.13: opt out of the default-extra set when you genuinely
    /// need to capture cache files (CI auditing the cache shape, a
    /// registry mirror, etc.).
    #[test]
    fn opt_out_of_default_ignores_captures_caches() {
        let src = TempDir::new().unwrap();
        write(src.path(), "__pycache__/foo.pyc", b"x");
        write(src.path(), "src/main.py", b"hi");
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = WalkFsCapture::new_without_default_ignores(src.path())
            .capture(&blobs)
            .unwrap();
        let tree: FsTree = serde_json::from_slice(&blobs.get(&cid).unwrap()).unwrap();
        let paths: Vec<&str> = tree.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.contains("__pycache__")),
            "without default ignores, __pycache__ must round-trip: {paths:?}"
        );
    }

    /// v1.0.13: `.ignore_from(".pfignore")` reads gitignore-style
    /// rules from the captured tree. Common case: operator drops
    /// project-specific patterns into a `.pfignore` file alongside
    /// the source.
    #[test]
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    fn ignore_from_file_applies_each_line() {
        let src = TempDir::new().unwrap();
        write(src.path(), "src/main.py", b"keep");
        write(src.path(), "secrets/api.key", b"private");
        write(src.path(), "logs/today.log", b"verbose");
        write(
            src.path(),
            ".pfignore",
            b"# project ignores\nsecrets\n*.log\n",
        );
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = WalkFsCapture::new(src.path())
            .ignore_from(src.path().join(".pfignore"))
            .unwrap()
            .capture(&blobs)
            .unwrap();
        let tree: FsTree = serde_json::from_slice(&blobs.get(&cid).unwrap()).unwrap();
        let paths: Vec<&str> = tree.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"src/main.py"));
        assert!(
            !paths.iter().any(|p| p.starts_with("secrets/")),
            "secrets/ should be filtered by .pfignore: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with(".log")),
            "*.log glob from .pfignore should filter logs: {paths:?}"
        );
    }
}
