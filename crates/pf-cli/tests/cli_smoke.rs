// SPDX-License-Identifier: MIT
//! End-to-end CLI smoke tests via `assert_cmd`. Boots the real `pf`
//! binary in a tempdir-rooted store and exercises every wired
//! subcommand.

use std::path::Path;

use assert_cmd::Command;
use pf_core::store::PfStore;
use predicates::str::contains;
use tempfile::TempDir;

fn pf(store: &Path) -> Command {
    let mut cmd = Command::cargo_bin("pf").expect("pf binary present in CARGO_BIN_EXE_pf");
    cmd.env("PF_STORE", store);
    cmd
}

fn make_sandbox(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src").join("main.py"), "print('hello')\n").unwrap();
    std::fs::write(root.join("README.md"), "# demo\n").unwrap();
}

#[test]
fn help_lists_every_subcommand() {
    let store = TempDir::new().unwrap();
    pf(store.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("snapshot"))
        .stdout(contains("fork"))
        .stdout(contains("checkout"))
        .stdout(contains("merge"))
        .stdout(contains("push"))
        .stdout(contains("pull"))
        .stdout(contains("clone"))
        .stdout(contains("log"))
        .stdout(contains("diff"))
        .stdout(contains("status"))
        .stdout(contains("gc"))
        .stdout(contains("verify"));
}

#[test]
fn snapshot_then_status_then_log() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());

    let out = pf(store.path())
        .args(["snapshot", "--agent-id", "test", "--fs-root"])
        .arg(sandbox.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let cid = String::from_utf8(out).unwrap().trim().to_owned();
    assert!(cid.starts_with("sha256:") && cid.len() == 71, "got {cid:?}");

    pf(store.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("manifests  : 1"));

    pf(store.path())
        .arg("log")
        .assert()
        .success()
        .stdout(contains(&cid));
}

#[test]
fn snapshot_then_checkout_round_trip() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());

    let cid = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    let target_root = TempDir::new().unwrap();
    let dst = target_root.path().join("restored");
    pf(store.path())
        .args(["checkout", &cid, "--into"])
        .arg(&dst)
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(dst.join("src").join("main.py")).unwrap(),
        "print('hello')\n"
    );
    assert_eq!(
        std::fs::read_to_string(dst.join("README.md")).unwrap(),
        "# demo\n"
    );
}

#[test]
fn fork_creates_n_children_with_distinct_cids() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let parent = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    let out = String::from_utf8(
        pf(store.path())
            .args(["fork", &parent, "-n", "3", "--explore", "test"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let lines: Vec<_> = out.lines().filter(|l| l.starts_with("sha256:")).collect();
    assert_eq!(lines.len(), 3);
    let unique: std::collections::HashSet<_> = lines.iter().copied().collect();
    assert_eq!(unique.len(), 3, "fork produced duplicate cids");
}

#[test]
fn merge_self_with_self_is_clean() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let cid = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    pf(store.path())
        .args(["merge", &cid, "--into", &cid])
        .assert()
        .success()
        .stdout(contains("clean"));
}

#[test]
fn diff_two_distinct_snapshots_shows_minus_plus_lines() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let a = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--name", "a", "--fs-root"])
            .arg(sandbox.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();
    // Mutate a file → different fs digest.
    std::fs::write(sandbox.path().join("README.md"), "# changed\n").unwrap();
    let b = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--name", "b", "--fs-root"])
            .arg(sandbox.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();
    assert_ne!(a, b);

    pf(store.path())
        .args(["diff", &a, &b])
        .assert()
        .success()
        .stdout(contains("- world.fs"))
        .stdout(contains("+ world.fs"));
}

#[test]
fn verify_passes_on_a_freshly_written_store() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    pf(store.path())
        .args(["snapshot", "--agent-id", "t", "--fs-root"])
        .arg(sandbox.path())
        .assert()
        .success();
    pf(store.path())
        .arg("verify")
        .assert()
        .success()
        .stdout(contains("0 bad"));
}

#[test]
fn gc_dry_run_reports_zero_unreachable_after_a_single_snapshot() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    pf(store.path())
        .args(["snapshot", "--agent-id", "t", "--fs-root"])
        .arg(sandbox.path())
        .assert()
        .success();
    pf(store.path())
        .args(["gc", "--dry-run"])
        .assert()
        .success()
        .stdout(contains("would delete"));
}

#[test]
fn push_to_hf_without_token_returns_clean_backend_error() {
    // v1.0.2: hf:// is live by default. Without a token + with a
    // bogus repo name, the HF API returns 401 Unauthorized which we
    // surface as a Backend error (exit code 1, not 2). The earlier
    // "exit code 2 = UnsupportedScheme" expectation only applied to
    // builds without --features hf-live.
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let cid = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();
    // Point HF at an unreachable endpoint so we never accidentally
    // hit real HF.huggingface.co during CI.
    pf(store.path())
        .env("HF_ENDPOINT", "http://127.0.0.1:1")
        .args(["push", &cid, "hf://test/repo"])
        .assert()
        .code(1)
        .stderr(contains("HF"));
}

#[test]
fn push_then_pull_via_file_registry_round_trips() {
    // Phase-9 acceptance: end-to-end registry round-trip via the local
    // FileRegistry backend.
    let store_a = TempDir::new().unwrap();
    let store_b = TempDir::new().unwrap();
    let registry = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());

    let cid = String::from_utf8(
        pf(store_a.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    let target = format!("file://{}", registry.path().display());
    pf(store_a.path())
        .args(["push", &cid, &target])
        .assert()
        .success();
    let pulled = String::from_utf8(
        pf(store_b.path())
            .args(["pull", &target])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();
    assert_eq!(pulled, cid, "round-trip CID identical");
}

#[test]
fn checkout_with_bad_cid_exits_1() {
    let store = TempDir::new().unwrap();
    pf(store.path())
        .args(["checkout", "not-a-cid", "--into"])
        .arg(store.path().join("x"))
        .assert()
        .code(1)
        .stderr(contains("bad cid"));
}

#[test]
fn completions_emits_a_shell_script() {
    let store = TempDir::new().unwrap();
    pf(store.path())
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(contains("_pf"));
}

// ---- v1.0.3 audit-fix CLI regression tests ----

/// v1.0.2 audit: --trace-from-jsonl with a missing path silently
/// captured an empty trace, then later broke `pf merge`. Should fail
/// at snapshot time.
#[test]
fn snapshot_with_missing_trace_path_fails_fast() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    pf(store.path())
        .args(["snapshot", "--agent-id", "t", "--fs-root"])
        .arg(sandbox.path())
        .args(["--trace-from-jsonl", "/nonexistent/trace.jsonl"])
        .assert()
        .failure()
        .stderr(contains("does not exist"));
}

/// v1.0.2 audit: snapshots after `pf checkout` had `parents: []`
/// so `pf merge` reported "no common ancestor". v1.0.3 writes a
/// `.pfcid` sentinel on checkout that snapshot autodetects as parent.
#[test]
fn checkout_then_snapshot_inherits_parent_cid_via_sentinel() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());

    // 1. Original snapshot.
    let cid = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    // 2. Check it out into a fresh dir.
    let restored = TempDir::new().unwrap();
    let into = restored.path().join("r");
    pf(store.path())
        .args(["checkout", &cid, "--into"])
        .arg(&into)
        .assert()
        .success();

    // The sentinel must exist + contain the CID.
    let pfcid = std::fs::read_to_string(into.join(".pfcid")).unwrap();
    assert_eq!(pfcid.trim(), cid);

    // 3. Snapshot the restored tree (no --parent flag passed).
    let edited_cid = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(&into)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    // 4. The new manifest should list cid as parent.
    let s = PfStore::open(store.path()).unwrap();
    let edited = s
        .get_manifest(&pf_core::digest::Digest256::parse(&edited_cid).unwrap())
        .unwrap();
    assert_eq!(
        edited.parents.len(),
        1,
        "post-checkout snapshot should inherit the parent CID via .pfcid"
    );
    assert_eq!(edited.parents[0].as_str(), cid);
}

/// v1.0.2 audit: `pf gc --retain-recent 1` deleted nested file blobs
/// inside the retained manifest's FsTree, breaking subsequent
/// `pf checkout`. v1.0.3 GC walks the transitive blob DAG.
#[test]
fn gc_retain_recent_does_not_orphan_fs_tree_blobs() {
    let store = TempDir::new().unwrap();
    let sandbox_a = TempDir::new().unwrap();
    make_sandbox(sandbox_a.path());
    let sandbox_b = TempDir::new().unwrap();
    make_sandbox(sandbox_b.path());
    // Write a different file content in B so its FsTree differs.
    std::fs::write(
        sandbox_b.path().join("README.md"),
        "# different demo content\n",
    )
    .unwrap();

    // Snapshot A then B (B is newer; retain_recent=1 keeps B, GCs A).
    let _cid_a = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox_a.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();
    // tiny sleep so created_at orders B after A.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let cid_b = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox_b.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    // GC keeping only the most recent (B). Pre-fix this deleted B's
    // file-content blobs because they sat inside its FsTree, not on
    // the manifest's top-level layer descriptors.
    pf(store.path())
        .args(["gc", "--retain-recent", "1"])
        .assert()
        .success();

    // Checkout B must still succeed end-to-end.
    let restore = TempDir::new().unwrap();
    let into = restore.path().join("r");
    pf(store.path())
        .args(["checkout", &cid_b, "--into"])
        .arg(&into)
        .assert()
        .success();
    assert_eq!(
        std::fs::read(into.join("README.md")).unwrap(),
        b"# different demo content\n",
        "GC must not clobber FsTree-nested blobs"
    );
}

/// v1.0.2 audit: env vars (including secrets) were captured verbatim
/// because `pf snapshot` exposed no --scrub-env flag. v1.0.3 wires it.
#[test]
fn snapshot_scrub_env_redacts_matching_keys() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let cid = String::from_utf8(
        pf(store.path())
            .env("MY_SECRET_TOKEN", "super-secret-value-do-not-leak")
            .env("PUBLIC_VAR", "this-is-fine")
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox.path())
            .args(["--scrub-env", "(?i)secret|token"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    let s = PfStore::open(store.path()).unwrap();
    let m = s
        .get_manifest(&pf_core::digest::Digest256::parse(&cid).unwrap())
        .unwrap();
    let env_bytes = s.blobs().get(&m.world.env).unwrap();
    let env_text = String::from_utf8_lossy(&env_bytes);
    assert!(
        !env_text.contains("super-secret-value-do-not-leak"),
        "scrub-env regex must redact the value of MY_SECRET_TOKEN; env blob was: {env_text}"
    );
    // Public var should still be present.
    assert!(
        env_text.contains("this-is-fine"),
        "non-matching var should be preserved"
    );
}
