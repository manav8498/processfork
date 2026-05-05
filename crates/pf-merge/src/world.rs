// SPDX-License-Identifier: MIT
//! World-layer merge: three-way file diff over `pf_world::FsTree` blobs.

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_world::{FsEntryKind, FsTree, FsTreeEntry};
use std::collections::BTreeMap;

/// One file path that needs human resolution. The merged tree contains a
/// blob with `<<<<<<<`-style markers; the path is also reported here so
/// the caller can surface a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldConflict {
    /// The conflicted path (relative to the world root).
    pub path: String,
    /// Digest of A's version of the file.
    pub a_digest: Digest256,
    /// Digest of B's version of the file.
    pub b_digest: Digest256,
    /// Digest of the common-ancestor version (or `None` if the file was
    /// added on both sides).
    pub x_digest: Option<Digest256>,
}

/// What [`merge_world`] returns.
#[derive(Clone, Debug)]
pub struct WorldMergeOutcome {
    /// Digest of the merged `fs.tree.v1` blob.
    pub merged_fs: Digest256,
    /// One entry per path that needed conflict markers.
    pub conflicts: Vec<WorldConflict>,
    /// Number of paths the merge handled cleanly (per the
    /// `agent_docs/merge-protocol.md` table).
    pub clean_paths: u32,
}

/// Three-way merge of three world-layer FS-tree blobs (`a`, `b`, `x`).
///
/// Algorithm matches the table in `agent_docs/merge-protocol.md`:
///
/// | A | B | X | outcome                                   |
/// |---|---|---|-------------------------------------------|
/// | = | = | = | unchanged (A's entry kept)                |
/// | ≠ | = | X | A wins (B didn't touch since X)           |
/// | = | ≠ | X | B wins (A didn't touch since X)           |
/// | ≠ | ≠ | X (and A == B) | identical change, no conflict    |
/// | ≠ | ≠ | X (and A ≠ B)  | CONFLICT: write `<<<<<<<` blob   |
/// | added on A only | A's entry                                       |
/// | added on B only | B's entry                                       |
/// | added on both same content | not a conflict                       |
/// | added on both different content | CONFLICT (no X side)            |
/// | deleted on one side | dropped                                     |
pub fn merge_world(
    blobs: &dyn BlobStore,
    a: &Digest256,
    b: &Digest256,
    x: &Digest256,
) -> pf_core::Result<WorldMergeOutcome> {
    let a_tree = read_tree(blobs, a)?;
    let b_tree = read_tree(blobs, b)?;
    let x_tree = read_tree(blobs, x)?;

    let a_map: BTreeMap<&str, &FsTreeEntry> = a_tree
        .entries
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let b_map: BTreeMap<&str, &FsTreeEntry> = b_tree
        .entries
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let x_map: BTreeMap<&str, &FsTreeEntry> = x_tree
        .entries
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();

    let mut all_paths: Vec<&str> = a_map
        .keys()
        .chain(b_map.keys())
        .chain(x_map.keys())
        .copied()
        .collect();
    all_paths.sort_unstable();
    all_paths.dedup();

    let mut merged: Vec<FsTreeEntry> = Vec::new();
    let mut conflicts: Vec<WorldConflict> = Vec::new();
    let mut clean_paths: u32 = 0;

    for path in all_paths {
        let in_a = a_map.get(path);
        let in_b = b_map.get(path);
        let in_x = x_map.get(path);

        match (in_a, in_b, in_x) {
            // Present in X but gone from BOTH sides → drop.
            (None, None, _) => {}
            // Present only in A.
            (Some(ea), None, None) => {
                merged.push((*ea).clone());
                clean_paths += 1;
            }
            // Present only in B.
            (None, Some(eb), None) => {
                merged.push((*eb).clone());
                clean_paths += 1;
            }
            // Existed in X, now in A only (B deleted).
            (Some(ea), None, Some(ex)) => {
                if entries_equal(ea, ex) {
                    // A == X (so B intended to delete a file A didn't
                    // touch) → honour B's deletion.
                    clean_paths += 1;
                } else {
                    // A modified, B deleted → conflict-style: keep A
                    // (delete-vs-modify resolves to keep the modify
                    // per spec; B's intent surfaces in the conflict log).
                    merged.push((*ea).clone());
                    conflicts.push(WorldConflict {
                        path: path.to_string(),
                        a_digest: ea.blob.clone().unwrap_or_else(zero_digest),
                        b_digest: zero_digest(),
                        x_digest: ex.blob.clone(),
                    });
                }
            }
            // Existed in X, now in B only (A deleted).
            (None, Some(eb), Some(ex)) => {
                if entries_equal(eb, ex) {
                    clean_paths += 1;
                } else {
                    merged.push((*eb).clone());
                    conflicts.push(WorldConflict {
                        path: path.to_string(),
                        a_digest: zero_digest(),
                        b_digest: eb.blob.clone().unwrap_or_else(zero_digest),
                        x_digest: ex.blob.clone(),
                    });
                }
            }
            // Present in both sides — apply the table.
            (Some(ea), Some(eb), x_e) => {
                let touched_a = x_e.is_none_or(|ex| !entries_equal(ea, ex));
                let touched_b = x_e.is_none_or(|ex| !entries_equal(eb, ex));
                match (touched_a, touched_b) {
                    (false, false) => {
                        // Neither side touched.
                        merged.push((*ea).clone());
                        clean_paths += 1;
                    }
                    (true, false) => {
                        // A wins.
                        merged.push((*ea).clone());
                        clean_paths += 1;
                    }
                    (false, true) => {
                        // B wins.
                        merged.push((*eb).clone());
                        clean_paths += 1;
                    }
                    (true, true) => {
                        if entries_equal(ea, eb) {
                            // Identical change.
                            merged.push((*ea).clone());
                            clean_paths += 1;
                        } else {
                            // CONFLICT.
                            let conflict_blob = build_conflict_blob(blobs, ea, eb)?;
                            let mut entry = (*ea).clone();
                            entry.blob = Some(conflict_blob);
                            // Size of the marker blob; recompute after build.
                            entry.size = blobs.get(entry.blob.as_ref().unwrap())?.len() as u64;
                            entry.kind = FsEntryKind::File;
                            entry.link_target = None;
                            merged.push(entry);
                            conflicts.push(WorldConflict {
                                path: path.to_string(),
                                a_digest: ea.blob.clone().unwrap_or_else(zero_digest),
                                b_digest: eb.blob.clone().unwrap_or_else(zero_digest),
                                x_digest: x_e.and_then(|ex| ex.blob.clone()),
                            });
                        }
                    }
                }
            }
        }
    }

    let merged_tree = FsTree {
        kind: "fs.tree.v1".into(),
        entries: merged,
    };
    let bytes = serde_json::to_vec(&merged_tree)?;
    Ok(WorldMergeOutcome {
        merged_fs: blobs.put(&bytes)?,
        conflicts,
        clean_paths,
    })
}

fn read_tree(blobs: &dyn BlobStore, digest: &Digest256) -> pf_core::Result<FsTree> {
    let bytes = blobs.get(digest)?;
    let t: FsTree = serde_json::from_slice(&bytes)?;
    if t.kind != "fs.tree.v1" {
        return Err(pf_core::Error::Integrity(format!(
            "expected fs.tree.v1, got {}",
            t.kind
        )));
    }
    Ok(t)
}

fn entries_equal(p: &FsTreeEntry, q: &FsTreeEntry) -> bool {
    // We only consider content-meaningful fields for equality; mode and
    // size are derived from blob/path/kind.
    p.kind == q.kind && p.blob == q.blob && p.link_target == q.link_target
}

/// Compose a textual `<<<<<<<` / `=======` / `>>>>>>>` marker blob from A
/// and B's file content. For binary or non-UTF-8 files, fall back to a
/// short header noting the digests.
fn build_conflict_blob(
    blobs: &dyn BlobStore,
    a: &FsTreeEntry,
    b: &FsTreeEntry,
) -> pf_core::Result<Digest256> {
    let a_text = match &a.blob {
        Some(d) => bytes_to_text(&blobs.get(d)?),
        None => String::new(),
    };
    let b_text = match &b.blob {
        Some(d) => bytes_to_text(&blobs.get(d)?),
        None => String::new(),
    };
    let composed = format!("<<<<<<< A\n{a_text}\n=======\n{b_text}\n>>>>>>> B\n");
    blobs.put(composed.as_bytes())
}

fn bytes_to_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => format!("<binary: {} bytes>", bytes.len()),
    }
}

fn zero_digest() -> Digest256 {
    Digest256::of(b"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::cas::MemBlobStore;

    fn entry(path: &str, content: &[u8], blobs: &dyn BlobStore) -> FsTreeEntry {
        let blob = blobs.put(content).unwrap();
        FsTreeEntry {
            path: path.to_string(),
            mode: "0644".into(),
            size: content.len() as u64,
            kind: FsEntryKind::File,
            blob: Some(blob),
            link_target: None,
        }
    }

    fn put_tree(blobs: &dyn BlobStore, entries: Vec<FsTreeEntry>) -> Digest256 {
        let t = FsTree {
            kind: "fs.tree.v1".into(),
            entries,
        };
        blobs.put(&serde_json::to_vec(&t).unwrap()).unwrap()
    }

    #[test]
    fn unchanged_files_pass_through_clean() {
        let blobs = MemBlobStore::new();
        let e = entry("a.txt", b"same", &blobs);
        let x = put_tree(&blobs, vec![e.clone()]);
        let a = put_tree(&blobs, vec![e.clone()]);
        let b = put_tree(&blobs, vec![e]);
        let r = merge_world(&blobs, &a, &b, &x).unwrap();
        assert!(r.conflicts.is_empty());
        assert_eq!(r.clean_paths, 1);
    }

    #[test]
    fn one_sided_change_wins() {
        let blobs = MemBlobStore::new();
        let original = entry("a.txt", b"original", &blobs);
        let modified = entry("a.txt", b"A's change", &blobs);
        let x = put_tree(&blobs, vec![original.clone()]);
        let a = put_tree(&blobs, vec![modified.clone()]);
        let b = put_tree(&blobs, vec![original]);
        let r = merge_world(&blobs, &a, &b, &x).unwrap();
        assert!(r.conflicts.is_empty());
        let merged = read_tree(&blobs, &r.merged_fs).unwrap();
        assert_eq!(merged.entries[0].blob, modified.blob);
    }

    #[test]
    fn identical_change_on_both_sides_is_clean() {
        let blobs = MemBlobStore::new();
        let original = entry("a.txt", b"v0", &blobs);
        let same = entry("a.txt", b"v1", &blobs);
        let x = put_tree(&blobs, vec![original]);
        let a = put_tree(&blobs, vec![same.clone()]);
        let b = put_tree(&blobs, vec![same]);
        let r = merge_world(&blobs, &a, &b, &x).unwrap();
        assert!(r.conflicts.is_empty());
        assert_eq!(r.clean_paths, 1);
    }

    #[test]
    fn divergent_change_produces_conflict_blob() {
        let blobs = MemBlobStore::new();
        let original = entry("a.txt", b"original\n", &blobs);
        let a_change = entry("a.txt", b"A wrote this\n", &blobs);
        let b_change = entry("a.txt", b"B wrote this\n", &blobs);
        let x = put_tree(&blobs, vec![original]);
        let a = put_tree(&blobs, vec![a_change.clone()]);
        let b = put_tree(&blobs, vec![b_change.clone()]);
        let r = merge_world(&blobs, &a, &b, &x).unwrap();
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!(r.conflicts[0].path, "a.txt");
        let merged = read_tree(&blobs, &r.merged_fs).unwrap();
        let conflict_blob = blobs.get(merged.entries[0].blob.as_ref().unwrap()).unwrap();
        let text = std::str::from_utf8(&conflict_blob).unwrap();
        assert!(text.contains("<<<<<<< A"));
        assert!(text.contains("A wrote this"));
        assert!(text.contains("======="));
        assert!(text.contains("B wrote this"));
        assert!(text.contains(">>>>>>> B"));
    }

    #[test]
    fn add_only_on_one_side_passes_through() {
        let blobs = MemBlobStore::new();
        let new_a = entry("new.txt", b"a's new file", &blobs);
        let x = put_tree(&blobs, vec![]);
        let a = put_tree(&blobs, vec![new_a.clone()]);
        let b = put_tree(&blobs, vec![]);
        let r = merge_world(&blobs, &a, &b, &x).unwrap();
        assert!(r.conflicts.is_empty());
        let merged = read_tree(&blobs, &r.merged_fs).unwrap();
        assert_eq!(merged.entries.len(), 1);
        assert_eq!(merged.entries[0].path, "new.txt");
    }

    #[test]
    fn add_on_both_with_same_content_is_clean() {
        let blobs = MemBlobStore::new();
        let new_same = entry("new.txt", b"same content", &blobs);
        let x = put_tree(&blobs, vec![]);
        let a = put_tree(&blobs, vec![new_same.clone()]);
        let b = put_tree(&blobs, vec![new_same]);
        let r = merge_world(&blobs, &a, &b, &x).unwrap();
        assert!(r.conflicts.is_empty());
    }

    #[test]
    fn add_on_both_with_different_content_conflicts() {
        let blobs = MemBlobStore::new();
        let na = entry("new.txt", b"A", &blobs);
        let nb = entry("new.txt", b"B", &blobs);
        let x = put_tree(&blobs, vec![]);
        let a = put_tree(&blobs, vec![na]);
        let b = put_tree(&blobs, vec![nb]);
        let r = merge_world(&blobs, &a, &b, &x).unwrap();
        assert_eq!(r.conflicts.len(), 1);
    }

    #[test]
    fn deleted_on_both_drops_path() {
        let blobs = MemBlobStore::new();
        let e = entry("gone.txt", b"bye", &blobs);
        let x = put_tree(&blobs, vec![e]);
        let a = put_tree(&blobs, vec![]);
        let b = put_tree(&blobs, vec![]);
        let r = merge_world(&blobs, &a, &b, &x).unwrap();
        let merged = read_tree(&blobs, &r.merged_fs).unwrap();
        assert!(merged.entries.is_empty());
    }
}
