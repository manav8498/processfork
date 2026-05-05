# SPDX-License-Identifier: MIT
"""Smoke tests for the AutoGen runtime adapter."""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest

pytest.importorskip("processfork", reason="run after `maturin develop -m crates/pf-py/Cargo.toml --features extension-module`")

from processfork_autogen import processfork_runtime  # noqa: E402


def test_register_and_record_messages(tmp_path: Path) -> None:
    rt = processfork_runtime(tmp_path / "store", fs_root=tmp_path / "fs")
    (tmp_path / "fs").mkdir()
    rt.record_message("alice", "user", "hi bob")
    rt.record_message("bob", "assistant", "hi alice")
    assert "alice" in rt.agents
    assert "bob" in rt.agents
    assert rt.agents["alice"].messages[0]["content"] == "hi bob"


def test_snapshot_flattens_per_agent_attribution(tmp_path: Path) -> None:
    import processfork as pf

    fs_root = tmp_path / "fs"
    fs_root.mkdir()
    (fs_root / "shared.md").write_text("# meeting notes\n")

    rt = processfork_runtime(tmp_path / "store", fs_root=fs_root)
    rt.record_message("alice", "user", "propose vote")
    rt.record_message("bob", "assistant", "I vote yes")
    rt.record_message("carol", "assistant", "I vote no")

    cid = rt.snapshot("post-vote")
    assert cid.startswith("sha256:")
    manifest = pf.read_manifest(rt.store, cid)
    assert manifest["agent"]["kind"] == "autogen-team"


def test_fork_via_cli(tmp_path: Path) -> None:
    if not shutil.which("pf"):
        pytest.skip("`pf` binary not on PATH; build with cargo build --release -p processfork")
    fs_root = tmp_path / "fs"
    fs_root.mkdir()
    rt = processfork_runtime(tmp_path / "store", fs_root=fs_root)
    rt.record_message("alice", "user", "go")
    cid = rt.snapshot("v0")
    forks = rt.fork(cid, n=3)
    assert len(forks) == 3
    assert all(f.startswith("sha256:") for f in forks)


def test_tool_call_recording_classifies(tmp_path: Path) -> None:
    fs_root = tmp_path / "fs"
    fs_root.mkdir()
    rt = processfork_runtime(tmp_path / "store", fs_root=fs_root)
    rt.record_tool_call("bob", "send_email", {"to": "x@y"}, {"sent": True})
    assert rt.agents["bob"].tool_calls[0]["side_effect_class"] == "irreversible"
