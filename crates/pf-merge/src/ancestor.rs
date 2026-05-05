// SPDX-License-Identifier: MIT
//! Lowest-common-ancestor discovery on the manifest parents DAG.

use pf_core::digest::Digest256;
use pf_core::store::PfStore;
use std::collections::{HashSet, VecDeque};

/// Explicit error type so the engine can distinguish "no LCA found" from
/// "found multiple parents in v1 (octopus)".
#[derive(Debug, thiserror::Error)]
pub enum AncestorError {
    /// The two images do not share a common ancestor in this store.
    #[error("no common ancestor between {a} and {b}")]
    NoCommonAncestor {
        /// First image.
        a: Digest256,
        /// Second image.
        b: Digest256,
    },
    /// One of the manifests reachable from a or b has more than two
    /// parents — octopus merges aren't supported in v1.
    #[error("multi-parent (octopus) ancestor at {at}; v1 merge supports at most 2 parents")]
    OctopusUnsupported {
        /// Manifest with > 2 parents.
        at: Digest256,
    },
    /// Wrapped store error.
    #[error("store: {0}")]
    Store(#[from] pf_core::Error),
}

/// Find the lowest common ancestor of `a` and `b` in `store`'s manifest DAG.
///
/// Walks both ancestor chains breadth-first. Returns the first manifest
/// digest reached from one chain that's already been visited by the other.
/// Trivial cases:
/// - `a == b` returns `a`.
/// - `a` is reachable from `b` (or vice-versa) returns `a` (resp. `b`).
pub fn find_lca(store: &PfStore, a: &Digest256, b: &Digest256) -> Result<Digest256, AncestorError> {
    if a == b {
        return Ok(a.clone());
    }
    let mut frontier_a: VecDeque<Digest256> = VecDeque::from([a.clone()]);
    let mut frontier_b: VecDeque<Digest256> = VecDeque::from([b.clone()]);
    let mut visited_a: HashSet<Digest256> = HashSet::from([a.clone()]);
    let mut visited_b: HashSet<Digest256> = HashSet::from([b.clone()]);

    loop {
        // Step A's frontier one level.
        if let Some(node) = frontier_a.pop_front() {
            let m = store.get_manifest(&node)?;
            if m.parents.len() > 2 {
                return Err(AncestorError::OctopusUnsupported { at: node });
            }
            for p in &m.parents {
                if visited_b.contains(p) {
                    return Ok(p.clone());
                }
                if visited_a.insert(p.clone()) {
                    frontier_a.push_back(p.clone());
                }
            }
        }
        // Step B's frontier one level.
        if let Some(node) = frontier_b.pop_front() {
            let m = store.get_manifest(&node)?;
            if m.parents.len() > 2 {
                return Err(AncestorError::OctopusUnsupported { at: node });
            }
            for p in &m.parents {
                if visited_a.contains(p) {
                    return Ok(p.clone());
                }
                if visited_b.insert(p.clone()) {
                    frontier_b.push_back(p.clone());
                }
            }
        }
        if frontier_a.is_empty() && frontier_b.is_empty() {
            return Err(AncestorError::NoCommonAncestor {
                a: a.clone(),
                b: b.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use pf_core::manifest::{
        AgentInfo, CacheLayer, EffectsLayer, MEDIATYPE_V1, Manifest, ModelLayer, TraceLayer,
        WorldLayer,
    };
    use tempfile::TempDir;

    fn put(store: &PfStore, parents: Vec<Digest256>, fingerprint: &str) -> Digest256 {
        let d = store.blobs().put(fingerprint.as_bytes()).unwrap();
        let m = Manifest {
            schema_version: 1,
            media_type: MEDIATYPE_V1.to_owned(),
            agent: AgentInfo {
                kind: "test".into(),
                version: "0".into(),
                fingerprint: fingerprint.into(),
            },
            model: ModelLayer {
                base: d.clone(),
                diff: d.clone(),
            },
            cache: CacheLayer {
                layout: "paged-batchinvariant-v1".into(),
                manifest: d.clone(),
            },
            world: WorldLayer {
                fs: d.clone(),
                env: d.clone(),
                procs: d.clone(),
            },
            effects: EffectsLayer { ledger: d.clone() },
            trace: TraceLayer { messages: d },
            created_at: Utc::now(),
            parents,
        };
        store.put_manifest(&m).unwrap()
    }

    #[test]
    fn lca_of_self_is_self() {
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let x = put(&store, vec![], "x");
        assert_eq!(find_lca(&store, &x, &x).unwrap(), x);
    }

    #[test]
    fn lca_of_simple_fork() {
        // x → a → c
        //  \→ b → d
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let x = put(&store, vec![], "x");
        let a = put(&store, vec![x.clone()], "a");
        let b = put(&store, vec![x.clone()], "b");
        let c = put(&store, vec![a], "c");
        let d = put(&store, vec![b], "d");
        let lca = find_lca(&store, &c, &d).unwrap();
        assert_eq!(lca, x);
    }

    #[test]
    fn lca_when_one_is_ancestor_of_other() {
        // x → a → b
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let x = put(&store, vec![], "x");
        let a = put(&store, vec![x.clone()], "a");
        let b = put(&store, vec![a.clone()], "b");
        // LCA of x and b is x (x is reachable from b through a).
        assert_eq!(find_lca(&store, &x, &b).unwrap(), x);
        // LCA of a and b is a.
        assert_eq!(find_lca(&store, &a, &b).unwrap(), a);
    }

    #[test]
    fn no_common_ancestor_errors() {
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let a = put(&store, vec![], "a");
        let b = put(&store, vec![], "b");
        assert!(matches!(
            find_lca(&store, &a, &b).unwrap_err(),
            AncestorError::NoCommonAncestor { .. }
        ));
    }

    #[test]
    fn octopus_rejected() {
        let dir = TempDir::new().unwrap();
        let store = PfStore::open(dir.path()).unwrap();
        let p1 = put(&store, vec![], "p1");
        let p2 = put(&store, vec![], "p2");
        let p3 = put(&store, vec![], "p3");
        let octopus = put(&store, vec![p1, p2, p3], "octopus");
        let other = put(&store, vec![], "other");
        let err = find_lca(&store, &octopus, &other).unwrap_err();
        assert!(matches!(err, AncestorError::OctopusUnsupported { .. }));
    }
}
