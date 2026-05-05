# SPDX-License-Identifier: MIT
"""Drops the three slash-command files into ``~/.claude/commands/processfork/``.

Each file is a simple Markdown body that Claude Code interprets as a
slash-command. The body shells out to the ProcessFork CLI (`pf`).
"""

from __future__ import annotations

from pathlib import Path
from typing import Mapping


_SNAPSHOT_BODY = """\
---
description: Capture the current Claude Code session into a `.pfimg`.
---

Run `pf snapshot --agent-id claude-code --fs-root .` and report the
resulting content-id back to me.
"""

_FORK_BODY = """\
---
description: Fork the current snapshot into N divergent branches.
---

Run `pf fork {{cid}} -n {{count|default:4}} --explore "{{hint|default:explore}}"`
and report each branch's content-id.
"""

_MERGE_BODY = """\
---
description: Three-way merge a sibling branch back into this one.
---

Run `pf merge {{from}} --into {{into|default:main}}` and surface any
world-layer conflicts to the user.
"""


def install_slash_commands(claude_dir: str | Path | None = None) -> Path:
    """Drop the three command files into ``$HOME/.claude/commands/processfork/``.

    Returns the directory the files were written to.
    """
    if claude_dir is None:
        claude_dir = Path.home() / ".claude"
    else:
        claude_dir = Path(claude_dir)
    out = claude_dir / "commands" / "processfork"
    out.mkdir(parents=True, exist_ok=True)
    files: Mapping[str, str] = {
        "snapshot.md": _SNAPSHOT_BODY,
        "fork.md": _FORK_BODY,
        "merge.md": _MERGE_BODY,
    }
    for name, body in files.items():
        (out / name).write_text(body)
    return out
