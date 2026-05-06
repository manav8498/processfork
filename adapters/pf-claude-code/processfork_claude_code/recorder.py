# SPDX-License-Identifier: MIT
"""Records a Claude Code session into a ProcessFork snapshot.

The recorder is a pure-Python data shuttle:

1. Claude Code's `PreToolUse` / `PostToolUse` hooks call
   :meth:`SessionRecorder.record_tool_call` with the tool name + JSON
   args + JSON result.
2. The orchestrator (typically the `pf-wrap-claude` shim) calls
   :meth:`SessionRecorder.snapshot` when the user issues `/snapshot`.
3. Snapshot writes the captured FS sandbox + env + chat trace into the
   local ProcessFork store via the Python SDK and returns the CID.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Mapping
import json
import os
import tempfile

from .classifier import ToolClassifier


@dataclass
class _ToolCall:
    tool: str
    args: Mapping[str, Any]
    result: Mapping[str, Any]
    side_effect_class: str


@dataclass
class SessionRecorder:
    """In-memory accumulator for a single Claude Code session.

    Snapshots emit a `.pfimg` whose:

    - **trace** = chat messages recorded via :meth:`record_message`
    - **world.fs** = the directory passed to :meth:`snapshot`
    - **effects.ledger** = the tool-call sequence (header + JSONL)
    """

    classifier: ToolClassifier = field(default_factory=ToolClassifier.default)
    messages: list[Mapping[str, str]] = field(default_factory=list)
    tool_calls: list[_ToolCall] = field(default_factory=list)

    def record_message(self, role: str, content: str) -> None:
        self.messages.append({"role": role, "content": content})

    def record_tool_call(
        self,
        tool: str,
        args: Mapping[str, Any],
        result: Mapping[str, Any],
    ) -> None:
        cls = self.classifier.classify(tool)
        self.tool_calls.append(_ToolCall(tool=tool, args=args, result=result, side_effect_class=cls))

    def ledger_jsonl(self) -> bytes:
        """Render the recorded tool calls as a `pf-effects` ledger blob.

        Returns the bytes the Python SDK would write into
        ``effects.ledger`` — header line plus one entry per call. The
        per-entry HMAC is left empty (`""`); a v1.1 enhancement will
        re-chain via :class:`pf_effects.ToolProxy` directly so the
        merge engine can still validate.
        """
        out = bytearray()
        out += json.dumps(
            {"kind": "effects.ledger.v1", "entries": len(self.tool_calls)}
        ).encode()
        out += b"\n"
        for ix, c in enumerate(self.tool_calls):
            entry = {
                "ts": "1970-01-01T00:00:00Z",  # SDK overwrites on snapshot
                "tool_id": c.tool,
                "args_hash": _sha256_of(json.dumps(c.args, sort_keys=True).encode()),
                "idempotency_key": f"01J{ix:013d}",
                "result_hash": _sha256_of(json.dumps(c.result, sort_keys=True).encode()),
                "side_effect_class": c.side_effect_class,
                "session_hmac": "",
            }
            out += json.dumps(entry).encode()
            out += b"\n"
        return bytes(out)

    def snapshot(
        self,
        store: Any,
        agent_kind: str,
        fs_root: str,
        env: Mapping[str, str] | None = None,
    ) -> str:
        """Snapshot the current session into ``store``. Returns the CID.

        ``store`` is a :class:`processfork.PfStore`; we don't import the
        SDK at module load so the recorder remains importable even
        without the cdylib built (helpful for unit-testing the
        classifier / ledger logic in isolation).
        """
        import processfork

        env_map = dict(env if env is not None else os.environ)
        # v1.0.4 audit fix: actually pass the recorded tool calls into
        # the snapshot's effects ledger so a restored agent sees them
        # as facts (ACRFence — "won't double-send your email"). v1.0.3
        # wired the SDK hook; this commit consumes it.
        effects_payload = [
            {
                "ix": ix,
                "tool_id": c.tool,
                "args_hash": _sha256_of(json.dumps(c.args, sort_keys=True).encode()),
                "result_hash": _sha256_of(
                    json.dumps(c.result, sort_keys=True, default=str).encode()
                ),
                "side_effect_class": c.side_effect_class,
                # idempotency key = sha256(tool + args) so re-issued
                # idempotent calls dedupe; irreversible ones use the
                # ix to make every issuance unique.
                "idempotency_key": _sha256_of(
                    f"{c.tool}:{json.dumps(c.args, sort_keys=True)}".encode()
                ),
            }
            for ix, c in enumerate(self.tool_calls)
        ]
        return processfork.snapshot_filesystem(
            store,
            agent_kind=agent_kind,
            fs_root=fs_root,
            env=env_map,
            messages=list(self.messages),
            effects=effects_payload,
        )


def _sha256_of(data: bytes) -> str:
    import hashlib

    return f"sha256:{hashlib.sha256(data).hexdigest()}"
