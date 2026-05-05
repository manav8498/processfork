# SPDX-License-Identifier: MIT
"""ProcessFork ↔ Claude Code integration.

Public API:

- :class:`ToolClassifier`: maps Claude-Code tool names → side-effect classes
  (defaults to a safe-by-default `Irreversible` for unknown tools).
- :class:`SessionRecorder`: records a Claude-Code session's chat trace +
  tool ledger; on demand, snapshots into a ProcessFork store.
- :func:`install_slash_commands`: drops the `/snapshot`, `/fork`, `/merge`
  command files into the user's `~/.claude/commands/processfork/` dir.
"""

from __future__ import annotations

from .classifier import ToolClassifier
from .recorder import SessionRecorder
from .install import install_slash_commands

__all__ = ["SessionRecorder", "ToolClassifier", "install_slash_commands"]
