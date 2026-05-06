# SPDX-License-Identifier: MIT
"""Smoke tests for the LangGraph checkpointer.

The tests pytest-skip cleanly when the `processfork` cdylib isn't
importable. They DO NOT require langgraph itself; the checkpointer is
duck-typed.
"""

from __future__ import annotations

from pathlib import Path

import pytest

pytest.importorskip("processfork", reason="run after `maturin develop -m crates/pf-py/Cargo.toml --features extension-module`")

from processfork_langgraph import ProcessForkCheckpointer, fork_thread  # noqa: E402


def test_put_then_get_returns_recent_checkpoint(tmp_path: Path) -> None:
    cp = ProcessForkCheckpointer(tmp_path / "store")
    a = cp.put("thread-x", {"step": 0, "msg": "hello"})
    b = cp.put("thread-x", {"step": 1, "msg": "more"})
    got = cp.get("thread-x")
    assert got is not None
    assert got.cid == b.cid != a.cid


def test_get_returns_real_state_not_manifest_placeholder(tmp_path: Path) -> None:
    """v1.0.2 audit: ProcessForkCheckpointer.get() returned a
    placeholder ``{"_manifest": ...}`` dict instead of the actual
    checkpoint state. v1.0.3 reads the trace blob and reconstitutes
    the original state dict end-to-end."""
    cp = ProcessForkCheckpointer(tmp_path / "store")
    state = {
        "step": 7,
        "messages": ["hello", "world"],
        "meta": {"temp": 0.7, "tools": ["bash"]},
    }
    cp.put("thread-1", state)
    got = cp.get("thread-1")
    assert got is not None
    assert "_manifest" not in got.state, (
        "v1.0.3: state must NOT be a manifest placeholder; got %r" % got.state
    )
    assert got.state == state


def test_list_yields_in_insertion_order(tmp_path: Path) -> None:
    cp = ProcessForkCheckpointer(tmp_path / "store")
    cids = [cp.put("t1", {"step": i}).cid for i in range(3)]
    listed = [c.cid for c in cp.list("t1")]
    assert listed == cids


def test_get_unknown_thread_returns_none(tmp_path: Path) -> None:
    cp = ProcessForkCheckpointer(tmp_path / "store")
    assert cp.get("never-recorded") is None


def test_distinct_threads_dont_cross_contaminate(tmp_path: Path) -> None:
    cp = ProcessForkCheckpointer(tmp_path / "store")
    a = cp.put("alpha", {"v": 1})
    b = cp.put("beta", {"v": 2})
    assert cp.get("alpha").cid == a.cid
    assert cp.get("beta").cid == b.cid


def test_fork_thread_via_cli(tmp_path: Path) -> None:
    # fork_thread shells out to the `pf` binary; skip if it's not on PATH.
    import shutil

    if not shutil.which("pf"):
        pytest.skip("`pf` binary not on PATH; build with `cargo build --release -p processfork` and add target/release to PATH")
    cp = ProcessForkCheckpointer(tmp_path / "store")
    cp.put("forkme", {"step": 0})
    forks = fork_thread(cp, "forkme", n=3, explore="explore")
    assert len(forks) == 3
    assert all(f.startswith("sha256:") for f in forks)
    # All cids must be distinct (different fingerprints → different manifests).
    assert len(set(forks)) == 3
