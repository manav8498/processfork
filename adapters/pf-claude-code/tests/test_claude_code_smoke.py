# SPDX-License-Identifier: MIT
"""Smoke tests for the Claude Code adapter.

Run with ``pytest adapters/pf-claude-code/tests/`` from the repo root.
The classifier and install tests have no external deps; the snapshot
test pytest-skips cleanly when ``processfork`` (the cdylib) isn't
importable.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from processfork_claude_code import (
    ToolClassifier,
    SessionRecorder,
    install_slash_commands,
)
from processfork_claude_code.classifier import (
    PURE,
    IRREVERSIBLE,
    NETWORK_ONLY,
)


# ----------------------- classifier -----------------------

def test_classifier_known_tools_get_correct_class() -> None:
    c = ToolClassifier.default()
    assert c.classify("Read") == PURE
    assert c.classify("Bash") == IRREVERSIBLE
    assert c.classify("WebFetch") == NETWORK_ONLY


def test_classifier_unknown_falls_back_to_irreversible() -> None:
    c = ToolClassifier.default()
    # Safe-by-default contract from agent_docs/integration-claude-code.md.
    assert c.classify("MysteryFutureToolFromV3") == IRREVERSIBLE


def test_classifier_user_table_overrides_default() -> None:
    c = ToolClassifier(table={"Bash": PURE}, fallback=PURE)
    assert c.classify("Bash") == PURE
    assert c.classify("UnknownTool") == PURE


# ----------------------- install -----------------------

def test_install_drops_three_files(tmp_path: Path) -> None:
    out = install_slash_commands(tmp_path / ".claude")
    assert (out / "snapshot.md").exists()
    assert (out / "fork.md").exists()
    assert (out / "merge.md").exists()
    body = (out / "snapshot.md").read_text()
    assert "pf snapshot" in body


def test_install_is_idempotent(tmp_path: Path) -> None:
    out1 = install_slash_commands(tmp_path / ".claude")
    out2 = install_slash_commands(tmp_path / ".claude")
    assert out1 == out2


# ----------------------- recorder (no SDK needed) -----------------------

def test_recorder_message_appends() -> None:
    r = SessionRecorder()
    r.record_message("user", "hi")
    r.record_message("assistant", "hello")
    assert len(r.messages) == 2
    assert r.messages[1]["role"] == "assistant"


def test_recorder_tool_call_classifies() -> None:
    r = SessionRecorder()
    r.record_tool_call("Bash", {"command": "ls"}, {"output": "..."})
    r.record_tool_call("Read", {"path": "x"}, {"contents": "..."})
    assert r.tool_calls[0].side_effect_class == IRREVERSIBLE
    assert r.tool_calls[1].side_effect_class == PURE


def test_recorder_ledger_jsonl_round_trips() -> None:
    import json

    r = SessionRecorder()
    r.record_tool_call("Read", {"path": "a"}, {"contents": "b"})
    r.record_tool_call("Bash", {"command": "ls"}, {"output": "x"})
    blob = r.ledger_jsonl()
    lines = [l for l in blob.split(b"\n") if l]
    header = json.loads(lines[0])
    assert header["kind"] == "effects.ledger.v1"
    assert header["entries"] == 2
    e0 = json.loads(lines[1])
    e1 = json.loads(lines[2])
    assert e0["tool_id"] == "Read"
    assert e0["side_effect_class"] == PURE
    assert e1["tool_id"] == "Bash"
    assert e1["side_effect_class"] == IRREVERSIBLE
    assert e0["args_hash"].startswith("sha256:")


# ----------------------- end-to-end snapshot via the SDK -----------------------

def test_snapshot_via_sdk(tmp_path: Path) -> None:
    pf = pytest.importorskip(
        "processfork",
        reason="run after `maturin develop -m crates/pf-py/Cargo.toml --features extension-module`",
    )
    sandbox = tmp_path / "sandbox"
    sandbox.mkdir()
    (sandbox / "main.py").write_text("print('hi')\n")

    r = SessionRecorder()
    r.record_message("user", "make it work")
    r.record_message("assistant", "done")
    r.record_tool_call("Read", {"path": "main.py"}, {"contents": "..."})

    store = pf.PfStore.open(str(tmp_path / "store"))
    cid = r.snapshot(store, agent_kind="claude-code", fs_root=str(sandbox))
    assert cid.startswith("sha256:") and len(cid) == 71
    manifest = pf.read_manifest(store, cid)
    assert manifest["agent"]["kind"] == "claude-code"

    # v1.0.4 audit-fix regression: the recorded tool call must
    # actually land in the on-disk effects ledger. v1.0.3 wired the
    # SDK hook; v1.0.4 connects it from the SessionRecorder.
    #
    # v1.0.9 update: the SDK now emits an HMAC-chained ledger via
    # `pf_effects::Ledger`, so the header carries
    # `session_secret_hex` + `verification_mode` (tamper-detection
    # mode) in addition to the v1 marker. We assert structural
    # invariants instead of an exact dict equality so future header
    # additions don't break this test.
    ledger_bytes = pf.read_blob(store, manifest["effects"]["ledger"])
    ledger_text = ledger_bytes.decode("utf-8")
    header_line, *entry_lines = ledger_text.strip().split("\n")
    import json as _json

    header = _json.loads(header_line)
    assert header.get("kind") == "effects.ledger.v1", (
        "ledger header missing v1 marker; got %r" % header
    )
    # v1.0.9: SDK-generated session secret embedded in tamper-detection
    # mode so `pf verify` can validate without an out-of-band secret.
    assert header.get("verification_mode") == "tamper-detection"
    assert "session_secret_hex" in header and len(header["session_secret_hex"]) >= 32
    assert len(entry_lines) == 1, (
        "expected exactly one entry (the recorded Read call); got %d"
        % len(entry_lines)
    )
    entry = _json.loads(entry_lines[0])
    assert entry["tool_id"] == "Read"
    assert entry["side_effect_class"] in {"pure", "idempotent"}
    assert entry["args_hash"].startswith("sha256:")
    assert entry["idempotency_key"].startswith("sha256:")
    # v1.0.9 ACRFence regression: every entry must carry a non-empty
    # session_hmac (the raw-JSONL bug used to leave them as "").
    assert entry.get("session_hmac"), "ledger entry missing HMAC chain link"
    assert len(entry["session_hmac"]) >= 32
