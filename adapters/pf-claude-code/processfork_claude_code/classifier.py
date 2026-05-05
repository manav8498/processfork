# SPDX-License-Identifier: MIT
"""Tool-name → side-effect class mapping.

Defaults are conservative: unknown tools → ``Irreversible`` so the replay
policy never accidentally re-issues something dangerous on restore.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Mapping


# Mirrors `pf-effects::SideEffectClass`. Strings keep the wire format
# stable across Python ↔ Rust without pulling pyo3 enums into the
# adapter.
PURE = "pure"
IDEMPOTENT = "idempotent"
IRREVERSIBLE = "irreversible"
NETWORK_ONLY = "network-only"


_DEFAULT_TOOLS: dict[str, str] = {
    # Read-only filesystem.
    "Read": PURE,
    "Glob": PURE,
    "Grep": PURE,
    "LS": PURE,
    # Write-side filesystem.
    "Write": IRREVERSIBLE,
    "Edit": IRREVERSIBLE,
    "NotebookEdit": IRREVERSIBLE,
    # Shell — assume worst-case.
    "Bash": IRREVERSIBLE,
    # Network reads.
    "WebFetch": NETWORK_ONLY,
    "WebSearch": NETWORK_ONLY,
    # Agentic / orchestration tools — pure for the wrapping agent's
    # ledger, since the *child* agent's own ledger captures effects.
    "Task": PURE,
    "TodoWrite": PURE,
}


@dataclass(frozen=True)
class ToolClassifier:
    """Maps a tool name to one of the four ``SideEffectClass`` strings.

    Construct from defaults via ``ToolClassifier.default()`` or from a
    user-supplied ``Mapping[str, str]`` (e.g. parsed from
    ``~/.processfork/tool-classes.toml``). Unknown tools fall back to
    :data:`IRREVERSIBLE` — the safe-by-default contract.
    """

    table: Mapping[str, str]
    fallback: str = IRREVERSIBLE

    @classmethod
    def default(cls) -> "ToolClassifier":
        return cls(table=dict(_DEFAULT_TOOLS))

    def classify(self, tool_name: str) -> str:
        return self.table.get(tool_name, self.fallback)
