# SPDX-License-Identifier: MIT
"""Smoke tests for the Python SDK.

Run with `maturin develop` then `pytest crates/pf-py/python/tests/`.
"""

from __future__ import annotations

import os
import tempfile
from pathlib import Path

import pytest

# pytest-skip cleanly if the cdylib hasn't been built yet (CI runs
# `maturin build` first; local devs do `maturin develop`).
processfork = pytest.importorskip(
    "processfork",
    reason="run `maturin develop -m crates/pf-py/Cargo.toml --features extension-module`",
)


def _make_sandbox(root: Path) -> None:
    (root / "src").mkdir(parents=True)
    (root / "src" / "main.py").write_text("print('hello')\n")
    (root / "README.md").write_text("# demo\n")


def test_digest_of_is_canonical() -> None:
    d = processfork.digest_of(b"")
    assert (
        d
        == "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    )


def test_pf_store_open_is_idempotent(tmp_path: Path) -> None:
    s1 = processfork.PfStore.open(str(tmp_path / "store"))
    s2 = processfork.PfStore.open(str(tmp_path / "store"))
    assert s1.physical_bytes() == s2.physical_bytes()


def test_snapshot_then_read_manifest(tmp_path: Path) -> None:
    sandbox = tmp_path / "sandbox"
    _make_sandbox(sandbox)
    store = processfork.PfStore.open(str(tmp_path / "store"))
    cid = processfork.snapshot_filesystem(
        store,
        agent_kind="test",
        fs_root=str(sandbox),
        env={"PWD": str(sandbox), "USER": "smoke"},
        messages=[
            {"role": "user", "content": "make it work"},
            {"role": "assistant", "content": "done"},
        ],
    )
    assert cid.startswith("sha256:") and len(cid) == 71
    manifest = processfork.read_manifest(store, cid)
    assert manifest["schema_version"] == 1
    assert manifest["agent"]["kind"] == "test"
    assert manifest["cache"]["layout"] == "paged-batchinvariant-v1"


def test_checkout_round_trip(tmp_path: Path) -> None:
    sandbox = tmp_path / "sandbox"
    _make_sandbox(sandbox)
    store = processfork.PfStore.open(str(tmp_path / "store"))
    cid = processfork.snapshot_filesystem(
        store,
        agent_kind="test",
        fs_root=str(sandbox),
        env={},
        messages=[],
    )
    target = tmp_path / "restored"
    processfork.checkout_filesystem(store, cid, str(target))
    assert (target / "src" / "main.py").read_text() == "print('hello')\n"
    assert (target / "README.md").read_text() == "# demo\n"


def test_default_scrub_redacts_secret_shaped_env(tmp_path: Path) -> None:
    """v1.0.9 audit fix: SDK must redact secret-shaped env-var names by
    default, even when the caller passes ``dict(os.environ)`` (which
    every adapter does). Prior versions wrote them verbatim, so a
    forgetful adapter author leaked OPENAI_API_KEY / GITHUB_TOKEN /
    DATABASE_PASSWORD into the .pfimg."""
    import json

    sandbox = tmp_path / "sandbox"
    _make_sandbox(sandbox)
    store = processfork.PfStore.open(str(tmp_path / "store"))
    cid = processfork.snapshot_filesystem(
        store,
        agent_kind="redact-test",
        fs_root=str(sandbox),
        env={
            "OPENAI_API_KEY": "sk-real-secret-must-not-appear",
            "GITHUB_TOKEN": "ghp_real-secret-must-not-appear",
            "DATABASE_PASSWORD": "real-secret-must-not-appear",
            "MY_API_KEY": "real-secret-must-not-appear",
            # Non-secret-shaped names must survive verbatim.
            "PWD": str(sandbox),
            "USER": "smoke",
        },
        messages=[],
    )
    manifest = processfork.read_manifest(store, cid)
    env_blob = json.loads(processfork.read_blob(store, manifest["world"]["env"]))
    vars_ = env_blob["vars"]
    assert vars_["OPENAI_API_KEY"] == "<redacted>"
    assert vars_["GITHUB_TOKEN"] == "<redacted>"
    assert vars_["DATABASE_PASSWORD"] == "<redacted>"
    assert vars_["MY_API_KEY"] == "<redacted>"
    assert vars_["USER"] == "smoke"
    # Hard guarantee: the secret value must NOT appear anywhere in the
    # serialized blob — not in vars, not in cwd, not in a stray field.
    raw = processfork.read_blob(store, manifest["world"]["env"])
    assert b"sk-real-secret-must-not-appear" not in raw
    assert b"ghp_real-secret-must-not-appear" not in raw
    assert b"real-secret-must-not-appear" not in raw


def test_default_scrub_can_be_disabled(tmp_path: Path) -> None:
    """Operators who genuinely need the raw env (rare; CI debugging)
    pass ``default_scrub_env=False``. Verify the opt-out works."""
    import json

    sandbox = tmp_path / "sandbox"
    _make_sandbox(sandbox)
    store = processfork.PfStore.open(str(tmp_path / "store"))
    cid = processfork.snapshot_filesystem(
        store,
        agent_kind="t",
        fs_root=str(sandbox),
        env={"OPENAI_API_KEY": "sk-test-value"},
        messages=[],
        default_scrub_env=False,
    )
    manifest = processfork.read_manifest(store, cid)
    env_blob = json.loads(processfork.read_blob(store, manifest["world"]["env"]))
    assert env_blob["vars"]["OPENAI_API_KEY"] == "sk-test-value"


def test_effects_ledger_is_hmac_chained(tmp_path: Path) -> None:
    """v1.0.9 audit fix: SDK ledger must be HMAC-chained (not raw JSONL).
    The chain header must carry the v1 marker plus a non-empty per-entry
    ``session_hmac``; tampering with any entry on disk must invalidate
    the chain. Prior versions wrote ``session_hmac = ""`` so reorder /
    tamper / delete was undetectable."""
    import json

    sandbox = tmp_path / "sandbox"
    _make_sandbox(sandbox)
    store = processfork.PfStore.open(str(tmp_path / "store"))
    cid = processfork.snapshot_filesystem(
        store,
        agent_kind="t",
        fs_root=str(sandbox),
        env={},
        messages=[],
        effects=[
            {
                "tool_id": "send_email",
                "args_hash": "sha256:" + "a" * 64,
                "result_hash": "sha256:" + "b" * 64,
                "idempotency_key": "msg-001",
                "side_effect_class": "irreversible",
            },
            {
                "tool_id": "git_push",
                "args_hash": "sha256:" + "c" * 64,
                "result_hash": "sha256:" + "d" * 64,
                "idempotency_key": "push-001",
                "side_effect_class": "irreversible",
            },
        ],
    )
    manifest = processfork.read_manifest(store, cid)
    raw = processfork.read_blob(store, manifest["effects"]["ledger"])
    header_line, _, body = raw.partition(b"\n")
    header = json.loads(header_line)
    assert header.get("kind") == "effects.ledger.v1"
    # Tamper-detection-mode header carries the embedded session secret.
    assert "session_secret_hex" in header
    assert header.get("verification_mode") == "tamper-detection"
    # Each entry's session_hmac must be a non-empty hex string — the
    # raw-JSONL bug left them as "".
    for line in body.splitlines():
        if not line.strip():
            continue
        entry = json.loads(line)
        assert entry.get("session_hmac"), (
            "v1.0.9 regression: ledger entry has empty session_hmac (raw-JSONL bug)"
        )
        assert len(entry["session_hmac"]) >= 32


def test_merge_two_forks_clean(tmp_path: Path) -> None:
    sandbox_a = tmp_path / "a"
    sandbox_b = tmp_path / "b"
    _make_sandbox(sandbox_a)
    _make_sandbox(sandbox_b)
    # Disjoint changes: A touches main.py, B touches README.md.
    (sandbox_a / "src" / "main.py").write_text("# A's edit\n")
    (sandbox_b / "README.md").write_text("# B's edit\n")
    sandbox_x = tmp_path / "x"
    _make_sandbox(sandbox_x)

    store = processfork.PfStore.open(str(tmp_path / "store"))
    cid_x = processfork.snapshot_filesystem(
        store, agent_kind="t", fs_root=str(sandbox_x), env={}, messages=[]
    )
    # For merge to find an LCA we need parents wired; the v1 SDK
    # snapshot_filesystem creates root manifests (no parents). Verify
    # the engine's no-LCA error surfaces cleanly.
    cid_a = processfork.snapshot_filesystem(
        store, agent_kind="t", fs_root=str(sandbox_a), env={}, messages=[]
    )
    cid_b = processfork.snapshot_filesystem(
        store, agent_kind="t", fs_root=str(sandbox_b), env={}, messages=[]
    )
    with pytest.raises(RuntimeError, match="no common ancestor"):
        processfork.merge(store, cid_a, cid_b)
    # Self-merge: ancestor = self, clean.
    report = processfork.merge(store, cid_a, cid_a)
    assert report["overall"] == "clean"
    assert report["ancestor"] == cid_a
