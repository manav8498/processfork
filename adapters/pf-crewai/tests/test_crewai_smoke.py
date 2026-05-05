# SPDX-License-Identifier: MIT
"""Smoke tests for the CrewAI memory adapter."""

from __future__ import annotations

from pathlib import Path

import pytest

pytest.importorskip("processfork", reason="run after `maturin develop -m crates/pf-py/Cargo.toml --features extension-module`")

from processfork_crewai import ProcessForkMemory  # noqa: E402


def test_save_returns_cid_and_appends(tmp_path: Path) -> None:
    fs_root = tmp_path / "fs"
    fs_root.mkdir()
    m = ProcessForkMemory(store_path=str(tmp_path / "store"), crew_id="x", fs_root=fs_root)
    cid = m.save({"task": "draft", "output": "v0"})
    assert cid.startswith("sha256:")
    assert m.snapshots == [cid]


def test_search_returns_recent_first(tmp_path: Path) -> None:
    fs_root = tmp_path / "fs"
    fs_root.mkdir()
    m = ProcessForkMemory(store_path=str(tmp_path / "store"), crew_id="x", fs_root=fs_root)
    cids = [m.save({"step": i}) for i in range(5)]
    hits = m.search("anything", limit=3)
    assert [h["cid"] for h in hits] == cids[-3:][::-1]


def test_reset_clears_snapshots(tmp_path: Path) -> None:
    fs_root = tmp_path / "fs"
    fs_root.mkdir()
    m = ProcessForkMemory(store_path=str(tmp_path / "store"), crew_id="x", fs_root=fs_root)
    m.save({})
    m.save({})
    m.reset()
    assert m.snapshots == []


def test_checkout_restores_into_tempdir(tmp_path: Path) -> None:
    fs_root = tmp_path / "fs"
    fs_root.mkdir()
    (fs_root / "draft.md").write_text("v0\n")
    m = ProcessForkMemory(store_path=str(tmp_path / "store"), crew_id="x", fs_root=fs_root)
    cid = m.save({"task": "write draft"})
    # Mutate the source.
    (fs_root / "draft.md").write_text("v1\n")
    # Restore the original.
    restored = m.checkout(cid)
    assert (restored / "draft.md").read_text() == "v0\n"
