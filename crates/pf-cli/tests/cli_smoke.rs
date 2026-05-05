// SPDX-License-Identifier: MIT
//! End-to-end CLI smoke tests via `assert_cmd`. Boots the real `pf`
//! binary in a tempdir-rooted store and exercises every wired
//! subcommand.

use std::path::Path;

use assert_cmd::Command;
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
fn push_exits_2_with_phase_9_message() {
    let store = TempDir::new().unwrap();
    pf(store.path())
        .args([
            "push",
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "hf://test/repo",
        ])
        .assert()
        .code(2)
        .stderr(contains("Phase 9"));
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
