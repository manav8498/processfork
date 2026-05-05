# SPDX-License-Identifier: MIT
"""examples/03-claude-code-fork — install slash-commands + snapshot a
synthetic Claude Code session.

Demonstrates the integration end-to-end without requiring a live
Claude Code session: we hand-construct a SessionRecorder, populate it
with a few messages and tool calls, snapshot it into a temp
ProcessFork store, and print the resulting CID.
"""

from __future__ import annotations

import tempfile
from pathlib import Path

import processfork as pf
from processfork_claude_code import SessionRecorder, install_slash_commands


def main() -> None:
    # 1. Install slash-commands into a temp Claude config.
    cfg = Path(tempfile.mkdtemp(prefix="claude-cfg-")) / ".claude"
    out = install_slash_commands(cfg)
    print(f"┌─ ProcessFork example 03: claude-code-fork ────────────────")
    print(f"│ slash-commands  : {out}")
    for f in sorted(out.iterdir()):
        print(f"│                  - {f.name}")

    # 2. Build a fake Claude Code session.
    sandbox = Path(tempfile.mkdtemp(prefix="sandbox-"))
    (sandbox / "main.py").write_text("print('hello')\n")
    (sandbox / "README.md").write_text("# demo project\n")

    rec = SessionRecorder()
    rec.record_message("user", "refactor main.py to use a function")
    rec.record_tool_call("Read", {"path": "main.py"}, {"contents": "print('hello')\n"})
    rec.record_tool_call(
        "Edit",
        {"path": "main.py", "old": "print('hello')", "new": "def main():\n    print('hello')\n\nmain()"},
        {"ok": True},
    )
    rec.record_message("assistant", "done — wrapped in a `main()` function.")

    # 3. Snapshot via the SDK.
    store = pf.PfStore.open(str(Path(tempfile.mkdtemp(prefix="store-"))))
    cid = rec.snapshot(store, agent_kind="claude-code", fs_root=str(sandbox))
    print(f"├──────────────────────────────────────────────────────────")
    print(f"│ session     : {len(rec.messages)} messages, {len(rec.tool_calls)} tool calls")
    print(f"│ snapshot    : {cid}")
    manifest = pf.read_manifest(store, cid)
    print(f"│ agent       : {manifest['agent']['kind']}@{manifest['agent']['version']}")
    print(f"│ trace blob  : {manifest['trace']['messages']}")
    print(f"│ world.fs    : {manifest['world']['fs']}")
    print("└─ ✓ Claude Code session snapshotted via the adapter")


if __name__ == "__main__":
    main()
