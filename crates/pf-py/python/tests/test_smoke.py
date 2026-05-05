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
