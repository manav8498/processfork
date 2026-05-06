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


def test_run_result_hash_distinguishes_outputs_past_truncation(tmp_path: Path) -> None:
    """v1.0.5 audit: hashing happened on the truncated string, so two
    outputs with identical first 8 KiB collided. v1.0.6 hashes the
    full bytes at record time, then truncates the displayed copy
    only.

    We force the FakeInterpreter to emit two strings that share the
    first 8 KiB but diverge in the trailing bytes; result_hash must
    differ.
    """

    class BigOutInterpreter:
        def __init__(self, suffix: str) -> None:
            self.messages: list[dict[str, str]] = []
            self._suffix = suffix
            self.computer = self  # we expose run() directly
            self._calls = 0

        def run(self, language: str, code: str) -> str:
            self._calls += 1
            return ("X" * 9000) + self._suffix

    a = BigOutInterpreter(suffix="A")
    wa = wrap_interpreter(a, store=tmp_path / "a", fs_root=tmp_path / "fa")
    (tmp_path / "fa").mkdir()
    wa.run("bash", "big A")

    b = BigOutInterpreter(suffix="DIFFERENT-TAIL-B")
    wb = wrap_interpreter(b, store=tmp_path / "b", fs_root=tmp_path / "fb")
    (tmp_path / "fb").mkdir()
    wb.run("bash", "big B")

    ha = wa._ledger[0]["result_hash"]
    hb = wb._ledger[0]["result_hash"]
    assert ha.startswith("sha256:") and hb.startswith("sha256:")
    assert ha != hb, (
        "result_hash must hash the FULL output, not the 8KiB-truncated display copy"
    )

    # Sanity: the displayed `result` IS truncated (so the ledger
    # stays small) and its truncation suffix advertises the size.
    assert "[truncated" in wa._ledger[0]["result"]


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
