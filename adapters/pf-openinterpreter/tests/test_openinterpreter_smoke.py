# SPDX-License-Identifier: MIT
"""Smoke tests for the OpenInterpreter wrapper.

Tests use a fake interpreter (no `open-interpreter` dep needed); the
SDK call is gated by the cdylib being importable.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pytest

pytest.importorskip("processfork", reason="run after `maturin develop -m crates/pf-py/Cargo.toml --features extension-module`")

from processfork_openinterpreter import wrap_interpreter  # noqa: E402


class FakeComputer:
    def __init__(self) -> None:
        self.calls: list[tuple[str, str]] = []

    def run(self, language: str, code: str) -> dict[str, Any]:
        self.calls.append((language, code))
        return {"output": f"ran {language}: {code[:24]}"}


class FakeInterpreter:
    def __init__(self) -> None:
        self.messages: list[dict[str, str]] = []
        self.computer = FakeComputer()

    def chat(self, prompt: str) -> dict[str, str]:
        self.messages.append({"role": "user", "content": prompt})
        reply = {"role": "assistant", "content": f"got it: {prompt[:24]}"}
        self.messages.append(reply)
        return reply


def test_wrap_creates_wrapper(tmp_path: Path) -> None:
    inner = FakeInterpreter()
    w = wrap_interpreter(inner, store=tmp_path / "store", fs_root=tmp_path)
    assert w.inner is inner
    assert w.fs_root == tmp_path


def test_chat_records_into_inner_messages(tmp_path: Path) -> None:
    inner = FakeInterpreter()
    w = wrap_interpreter(inner, store=tmp_path / "store", fs_root=tmp_path)
    w.chat("rm -rf /tmp/foo")
    assert len(inner.messages) == 2
    assert inner.messages[0]["role"] == "user"


def test_run_taps_ledger(tmp_path: Path) -> None:
    inner = FakeInterpreter()
    w = wrap_interpreter(inner, store=tmp_path / "store", fs_root=tmp_path)
    w.run("bash", "echo hi")
    w.run("python", "print('x')")
    assert len(w._ledger) == 2
    assert w._ledger[0]["tool"] == "oi.computer.run.bash"
    assert w._ledger[0]["side_effect_class"] == "irreversible"


def test_snapshot_then_checkout_round_trip(tmp_path: Path) -> None:
    sandbox = tmp_path / "sandbox"
    sandbox.mkdir()
    (sandbox / "data.txt").write_text("v0\n")

    inner = FakeInterpreter()
    inner.messages = [
        {"role": "user", "content": "make data.txt v1"},
        {"role": "assistant", "content": "ok"},
    ]
    w = wrap_interpreter(inner, store=tmp_path / "store", fs_root=sandbox)

    cid = w.snapshot("pre-edit")
    assert cid.startswith("sha256:")
    assert w.snapshots["pre-edit"] == cid

    # Mutate the sandbox.
    (sandbox / "data.txt").write_text("v1\n")

    # Restore the pre-edit snapshot into a fresh dir.
    restored = w.checkout("pre-edit", into=tmp_path / "restored")
    assert (restored / "data.txt").read_text() == "v0\n"


def test_checkout_unknown_name_errors(tmp_path: Path) -> None:
    w = wrap_interpreter(FakeInterpreter(), store=tmp_path / "store", fs_root=tmp_path)
    with pytest.raises(KeyError):
        w.checkout("never-snapshotted")
