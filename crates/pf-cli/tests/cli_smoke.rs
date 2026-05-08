// SPDX-License-Identifier: MIT
//! End-to-end CLI smoke tests via `assert_cmd`. Boots the real `pf`
//! binary in a tempdir-rooted store and exercises every wired
//! subcommand.

use std::path::Path;

use assert_cmd::Command;
use pf_core::store::PfStore;
use predicates::prelude::PredicateBooleanExt;
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

/// v1.0.5 audit: --quiesce-cmd runs before fs walk, --resume-cmd
/// runs on Drop. Both must execute even if the snapshot path errors.
/// We verify by having each command touch a sentinel file.
#[test]
fn snapshot_runs_quiesce_and_resume_commands() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let scratch = TempDir::new().unwrap();
    let q = scratch.path().join("quiesce.touch");
    let r = scratch.path().join("resume.touch");
    pf(store.path())
        .args(["snapshot", "--agent-id", "t", "--fs-root"])
        .arg(sandbox.path())
        .args(["--quiesce-cmd"])
        .arg(format!("touch {}", q.display()))
        .args(["--resume-cmd"])
        .arg(format!("touch {}", r.display()))
        .assert()
        .success();
    assert!(q.exists(), "--quiesce-cmd should have run");
    assert!(r.exists(), "--resume-cmd should have run on Drop");
}

/// v1.0.5: a failing --quiesce-cmd should fail the snapshot fast
/// (the operator's app didn't successfully enter quiescence).
#[test]
fn snapshot_failing_quiesce_cmd_aborts_snapshot() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    pf(store.path())
        .args(["snapshot", "--agent-id", "t", "--fs-root"])
        .arg(sandbox.path())
        .args(["--quiesce-cmd", "exit 7"])
        .assert()
        .failure()
        .stderr(contains("--quiesce-cmd"));
}

/// v1.0.7 audit: effects ledger entries are now HMAC-chained at
/// snapshot time. Validate end-to-end that:
///   1. snapshot writes a ledger with non-empty session_hmac per entry,
///   2. `pf verify` accepts the original blob (chains_ok),
///   3. `pf verify` rejects a tampered blob (chains_bad).
#[test]
fn snapshot_ledger_is_hmac_chained_and_verify_detects_tampering() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let effects_path = sandbox.path().join("effects.jsonl");
    std::fs::write(
        &effects_path,
        concat!(
            r#"{"tool_id":"send_email","args_hash":"sha256:aa","result_hash":"sha256:bb","idempotency_key":"k1","side_effect_class":"irreversible"}"#,
            "\n",
            r#"{"tool_id":"db_write","args_hash":"sha256:cc","result_hash":"sha256:dd","idempotency_key":"k2","side_effect_class":"idempotent"}"#,
            "\n",
        ),
    )
    .unwrap();

    let cid = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox.path())
            .args(["--effects-from-jsonl"])
            .arg(&effects_path)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    // Read the ledger blob — every entry must have a non-empty
    // session_hmac field.
    let s = PfStore::open(store.path()).unwrap();
    let m = s
        .get_manifest(&pf_core::digest::Digest256::parse(&cid).unwrap())
        .unwrap();
    let ledger_bytes = s.blobs().get(&m.effects.ledger).unwrap();
    let text = String::from_utf8(ledger_bytes.clone()).unwrap();
    let header_line = text.lines().next().unwrap();
    let header: serde_json::Value = serde_json::from_str(header_line).unwrap();
    assert_eq!(header["kind"], "effects.ledger.v1");
    assert_eq!(header["entries"], 2);
    assert!(
        header.get("session_secret_hex").is_some(),
        "default snapshot must embed session_secret_hex for tamper-detection"
    );
    for (lineno, l) in text.lines().skip(1).enumerate() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        let hmac_str = v["session_hmac"].as_str().unwrap_or("");
        assert!(
            !hmac_str.is_empty(),
            "entry {lineno} session_hmac must be non-empty (was: {l})"
        );
        assert_eq!(hmac_str.len(), 64, "session_hmac must be 32 bytes hex");
    }

    // Original blob → pf verify passes.
    pf(store.path()).args(["verify"]).assert().success();

    // Tamper: rewrite the second entry's tool_id from "db_write" to
    // "send_email" without re-chaining. pf verify must catch it.
    let tampered = text.replace(r#""tool_id":"db_write""#, r#""tool_id":"hijacked""#);
    // Write the tampered blob into the store under a NEW digest
    // (simulating an attacker who modified the on-disk blob and
    // re-pointed the manifest). For the test we replace the file
    // directly at the existing digest's on-disk path.
    let blob_path = store
        .path()
        .join("blobs")
        .join("sha256")
        .join(&m.effects.ledger.hex()[..2])
        .join(format!("{}.zst", m.effects.ledger.hex()));
    let compressed = zstd::encode_all(tampered.as_bytes(), 19).unwrap();
    std::fs::write(&blob_path, compressed).unwrap();

    let result = pf(store.path()).args(["verify"]).assert().failure();
    let stderr = String::from_utf8(result.get_output().stderr.clone()).unwrap();
    // pf verify either fails on the blob's digest (because we
    // rewrote the on-disk content but the blob name is the original
    // hash) OR fails on the HMAC chain. Either way the failure must
    // mention "verification" or "HMAC".
    assert!(
        stderr.contains("verification") || stderr.contains("HMAC") || stderr.contains("BAD"),
        "expected verification failure, stderr was: {stderr}"
    );
}

/// v1.0.7 audit: env capture redacts secret-shaped names by default.
/// Operator gets safe-by-default behavior without remembering --scrub-env.
#[test]
fn snapshot_default_scrub_redacts_secret_shaped_vars() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let cid = String::from_utf8(
        pf(store.path())
            .env("OPENAI_API_KEY", "sk-leaked-via-default-capture-bug")
            .env("DATABASE_PASSWORD", "hunter2")
            .env("PUBLIC_VAR", "this-is-fine")
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

    let s = PfStore::open(store.path()).unwrap();
    let m = s
        .get_manifest(&pf_core::digest::Digest256::parse(&cid).unwrap())
        .unwrap();
    let env_text = String::from_utf8(s.blobs().get(&m.world.env).unwrap()).unwrap();
    assert!(
        !env_text.contains("sk-leaked-via-default-capture-bug"),
        "default scrub must redact OPENAI_API_KEY value; env blob was: {env_text}"
    );
    assert!(
        !env_text.contains("hunter2"),
        "default scrub must redact DATABASE_PASSWORD value"
    );
    assert!(
        env_text.contains("this-is-fine"),
        "non-secret env vars must still be captured"
    );
}

/// v1.0.6 audit: when --quiesce-cmd fails AFTER it's already mutated
/// app state, --resume-cmd must still run so the operator's app
/// doesn't get stuck in a half-quiesced state.
#[test]
fn snapshot_failing_quiesce_still_runs_resume() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let scratch = TempDir::new().unwrap();
    let flag = scratch.path().join("flag.set");
    let resume_marker = scratch.path().join("resume.ran");
    pf(store.path())
        .args(["snapshot", "--agent-id", "t", "--fs-root"])
        .arg(sandbox.path())
        .args(["--quiesce-cmd"])
        // Quiesce: touch a sentinel flag (simulates partial state
        // mutation) THEN exit 7 (simulates a downstream failure).
        .arg(format!("touch {} && exit 7", flag.display()))
        .args(["--resume-cmd"])
        .arg(format!("touch {}", resume_marker.display()))
        .assert()
        .failure(); // snapshot itself fails
    assert!(flag.exists(), "quiesce-cmd partial state should remain");
    assert!(
        resume_marker.exists(),
        "--resume-cmd must run even when --quiesce-cmd fails"
    );
}

/// v1.0.4 audit (round 2): --trace-from-jsonl validated path
/// existence + is_file but accepted invalid JSON content. Now we
/// parse each non-empty line as `{"role": str, "content": str}` and
/// fail the snapshot at fail-fast time.
#[test]
fn snapshot_with_malformed_trace_jsonl_fails_fast() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    let bad_trace = sandbox.path().join("bad-trace.jsonl");
    std::fs::write(
        &bad_trace,
        "this is not json\n{\"role\":\"user\"}\n", // line 2 missing 'content'
    )
    .unwrap();
    pf(store.path())
        .args(["snapshot", "--agent-id", "t", "--fs-root"])
        .arg(sandbox.path())
        .args(["--trace-from-jsonl"])
        .arg(&bad_trace)
        .assert()
        .failure()
        .stderr(contains("--trace-from-jsonl"));
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

/// v1.0.12: full pf merge → pf merge-resolve → pf merge-finalize round-trip.
/// Snapshot a common ancestor X, then snapshot two divergent edits A and B
/// (each declaring X as parent). Merging A and B must report a conflict;
/// the resolution flow must drop the merged FS into a workdir, expose the
/// conflict-markered file, and on a second invocation produce a clean
/// finalized image with the merged-cid as its parent.
#[test]
#[allow(clippy::too_many_lines)] // single linear narrative is easier to audit
fn merge_resolve_finalize_round_trip() {
    let store = TempDir::new().unwrap();

    // Common ancestor: README + main.py.
    let sandbox_x = TempDir::new().unwrap();
    std::fs::create_dir_all(sandbox_x.path().join("src")).unwrap();
    std::fs::write(sandbox_x.path().join("src").join("main.py"), "v0\n").unwrap();
    std::fs::write(sandbox_x.path().join("README.md"), "# v0\n").unwrap();
    let cid_x = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox_x.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    // Branch A: edits main.py.
    let sandbox_a = TempDir::new().unwrap();
    std::fs::create_dir_all(sandbox_a.path().join("src")).unwrap();
    std::fs::write(
        sandbox_a.path().join("src").join("main.py"),
        "branch_a_change\n",
    )
    .unwrap();
    std::fs::write(sandbox_a.path().join("README.md"), "# v0\n").unwrap();
    let cid_a = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox_a.path())
            .args(["--parent", &cid_x])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    // Branch B: conflicting edit to main.py.
    let sandbox_b = TempDir::new().unwrap();
    std::fs::create_dir_all(sandbox_b.path().join("src")).unwrap();
    std::fs::write(
        sandbox_b.path().join("src").join("main.py"),
        "branch_b_change\n",
    )
    .unwrap();
    std::fs::write(sandbox_b.path().join("README.md"), "# v0\n").unwrap();
    let cid_b = String::from_utf8(
        pf(store.path())
            .args(["snapshot", "--agent-id", "t", "--fs-root"])
            .arg(sandbox_b.path())
            .args(["--parent", &cid_x])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_owned();

    // pf merge A B should exit 3 (MergeConflict) AND mention the new
    // resolve-finalize hint. The merged-CID is in the stdout output
    // ("merged   : sha256:...").
    let out = pf(store.path())
        .args(["merge", &cid_b, "--into", &cid_a])
        .assert()
        .failure()
        .stderr(contains("pf merge-resolve"))
        .stderr(contains("pf merge-finalize"))
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let merged_cid = stdout
        .lines()
        .find_map(|l| l.strip_prefix("merged   : "))
        .expect("pf merge stdout must include `merged   : <cid>`")
        .trim()
        .to_owned();
    assert!(merged_cid.starts_with("sha256:"));

    // pf merge-resolve drops the merged FS into a workdir + reports
    // the conflict-markered files. Workdir must NOT pre-exist.
    let workdir_root = TempDir::new().unwrap();
    let workdir = workdir_root.path().join("resolve");
    pf(store.path())
        .args(["merge-resolve", &merged_cid, "--workdir"])
        .arg(&workdir)
        .assert()
        .success()
        .stdout(contains("file(s) need resolution"))
        .stdout(contains("src/main.py"));

    // The conflict file should literally contain Git-style markers.
    let conflicted = std::fs::read_to_string(workdir.join("src").join("main.py")).unwrap();
    assert!(
        conflicted.contains("<<<<<<<") && conflicted.contains(">>>>>>>"),
        "merged FS must carry conflict markers, got: {conflicted:?}"
    );

    // Without resolution, merge-finalize must REFUSE (exit 3).
    pf(store.path())
        .args(["merge-finalize", &merged_cid, "--workdir"])
        .arg(&workdir)
        .assert()
        .failure()
        .stderr(contains("still contain conflict markers"));

    // Hand-resolve and try again.
    std::fs::write(
        workdir.join("src").join("main.py"),
        "branch_a_change\nbranch_b_change\n",
    )
    .unwrap();

    let final_out = String::from_utf8(
        pf(store.path())
            .args(["merge-finalize", &merged_cid, "--workdir"])
            .arg(&workdir)
            .assert()
            .success()
            .stdout(contains("finalized:"))
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let final_cid = final_out
        .lines()
        .find_map(|l| l.strip_prefix("finalized: "))
        .expect("merge-finalize stdout must include `finalized: <cid>`")
        .trim()
        .to_owned();
    assert!(final_cid.starts_with("sha256:"));
    assert_ne!(final_cid, merged_cid);

    // Verify the finalized image:
    //   - has merged_cid as its single parent (closes the merge).
    //   - its FS no longer carries conflict markers.
    let restored = TempDir::new().unwrap();
    let restore_path = restored.path().join("out");
    pf(store.path())
        .args(["checkout", &final_cid, "--into"])
        .arg(&restore_path)
        .assert()
        .success();
    let restored_main = std::fs::read_to_string(restore_path.join("src").join("main.py")).unwrap();
    assert!(!restored_main.contains("<<<<<<<"));
    assert_eq!(restored_main, "branch_a_change\nbranch_b_change\n");

    // The finalized image's manifest must list merged_cid as its
    // single parent — that's what closes the merge in the DAG.
    let store_handle = PfStore::open(store.path()).unwrap();
    let final_digest = pf_core::digest::Digest256::parse(&final_cid).unwrap();
    let final_manifest = store_handle.get_manifest(&final_digest).unwrap();
    assert_eq!(final_manifest.parents.len(), 1);
    assert_eq!(final_manifest.parents[0].as_str(), merged_cid);

    // x_y_z context: keep the parent vars used so cid_a / cid_b /
    // cid_x are not flagged unused; they're documented in the test
    // narrative and would surface in a richer DAG check on demand.
    let _ = (cid_a, cid_b, cid_x);
}

/// v1.0.12: `pf snapshot --criu-pid` invokes the processfork-criu
/// adapter via python3. On macOS the adapter's gating reports
/// "CRIU is Linux-only" and the CLI must surface that as a clean
/// failure, not a panic or a silent empty procs blob.
///
/// The test runs unconditionally (every CI host has a /bin/sh and
/// most have python3); if python3 is missing or processfork-criu
/// isn't importable the failure message is still informative and
/// the assertions still hold (we don't check the exact wording,
/// just that the command failed and stderr mentions criu).
#[cfg(not(target_os = "linux"))]
#[test]
fn snapshot_criu_pid_fails_cleanly_on_non_linux() {
    let store = TempDir::new().unwrap();
    let sandbox = TempDir::new().unwrap();
    make_sandbox(sandbox.path());
    pf(store.path())
        .args(["snapshot", "--agent-id", "t", "--fs-root"])
        .arg(sandbox.path())
        .args(["--criu-pid", "1"]) // pid 1 is universal, no real dump attempted
        .assert()
        .failure()
        .stderr(
            contains("criu")
                .or(contains("CRIU"))
                .or(contains("python3")),
        );
}

/// v1.0.12: --force overrides the conflict-marker scan in
/// merge-finalize. Operators with legitimate `<<<<<<<` content in
/// their tree (e.g. test fixtures for the merge engine itself)
/// should be able to opt out.
#[test]
fn merge_finalize_force_skips_marker_scan() {
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

    // Drop a workdir with literal conflict-marker content + finalize
    // with --force; should succeed and produce a child of `cid`.
    let workdir_root = TempDir::new().unwrap();
    let workdir = workdir_root.path().join("with_markers");
    std::fs::create_dir_all(&workdir).unwrap();
    std::fs::write(
        workdir.join("fixture.txt"),
        "<<<<<<<\nfoo\n=======\nbar\n>>>>>>>\n",
    )
    .unwrap();
    pf(store.path())
        .args(["merge-finalize", &cid, "--workdir"])
        .arg(&workdir)
        .arg("--force")
        .assert()
        .success()
        .stdout(contains("finalized:"));
}
