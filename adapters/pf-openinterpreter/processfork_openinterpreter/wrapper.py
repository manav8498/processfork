# SPDX-License-Identifier: MIT
"""Wraps an OpenInterpreter instance with snapshot / checkout / chat-with-tap.

Duck-typed: doesn't import `interpreter` at module load. Tests cover
the wrapper logic against a fake interpreter that exposes
``messages: list`` and ``computer.run(language, code)``.
"""

from __future__ import annotations

import os
import shutil
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class WrappedInterpreter:
    """A thin shell around an OpenInterpreter object that records snapshots
    and lets you ``checkout(name)`` to restore the FS sandbox.

    Snapshot bookkeeping is in-memory by `name → CID`. Persistent
    naming is via the underlying ProcessFork store (every snapshot has
    a stable CID; the in-memory `name` map is just convenience).
    """

    inner: Any  # the wrapped `interpreter` module / object
    store: Any  # processfork.PfStore
    fs_root: Path
    snapshots: dict[str, str] = field(default_factory=dict)
    _ledger: list[dict[str, Any]] = field(default_factory=list)

    # ---- snapshot / restore ----

    def snapshot(self, name: str) -> str:
        import hashlib
        import json

        import processfork

        messages = list(self._messages_from_inner())
        # v1.0.4 audit fix: pass the recorded ledger entries through to
        # the snapshot so a restored interpreter sees its prior tool
        # calls as facts, not opportunities to re-issue.
        effects_payload = [
            {
                "ix": ix,
                "tool_id": e["tool"],
                "args_hash": "sha256:"
                + hashlib.sha256(
                    json.dumps(e.get("args", {}), sort_keys=True).encode()
                ).hexdigest(),
                # v1.0.6 audit: prefer the pre-computed result_hash
                # (computed from the FULL output before truncation).
                # Fall back to hashing the (already-truncated) result
                # only if a caller built the ledger entry manually.
                "result_hash": e.get(
                    "result_hash",
                    "sha256:"
                    + hashlib.sha256(
                        json.dumps(e.get("result", ""), sort_keys=True, default=str).encode()
                    ).hexdigest(),
                ),
                "side_effect_class": e.get("side_effect_class", "irreversible"),
                "idempotency_key": "sha256:"
                + hashlib.sha256(
                    f"{e['tool']}:{json.dumps(e.get('args', {}), sort_keys=True)}".encode()
                ).hexdigest(),
            }
            for ix, e in enumerate(self._ledger)
        ]
        cid = processfork.snapshot_filesystem(
            self.store,
            agent_kind="open-interpreter",
            fs_root=str(self.fs_root),
            env=dict(os.environ),
            messages=messages,
            effects=effects_payload,
        )
        self.snapshots[name] = cid
        return cid

    def checkout(self, name: str, into: str | os.PathLike[str] | None = None) -> Path:
        """Restore the snapshot named ``name``.

        ``into``: where to materialise the FS tree. If ``None``, we
        restore over a fresh temp dir (the wrapper does NOT clobber
        ``fs_root`` automatically — operator opt-in only).
        """
        import processfork

        cid = self.snapshots.get(name)
        if cid is None:
            raise KeyError(f"no snapshot named {name!r}; known: {sorted(self.snapshots)}")
        target = Path(into) if into is not None else Path(tempfile.mkdtemp(prefix=f"oi-restore-{name}-"))
        # restore_tree refuses to overwrite an existing path, so if
        # `into` exists we move it aside first.
        if target.exists():
            backup = target.with_suffix(target.suffix + ".pf-bak")
            if backup.exists():
                shutil.rmtree(backup)
            shutil.move(str(target), str(backup))
        processfork.checkout_filesystem(self.store, cid, str(target))
        return target

    # ---- chat-with-tap ----

    def chat(self, prompt: str) -> Any:
        """Forward to the wrapped interpreter's `.chat`, recording the
        prompt + assistant response into the wrapper's local message
        log."""
        result = self.inner.chat(prompt)
        # OpenInterpreter mutates `interpreter.messages` in place. We
        # tap that on the next snapshot.
        return result

    def run(self, language: str, code: str) -> Any:
        """Forward to `interpreter.computer.run`, recording the call
        and its result into the wrapper's effect ledger.

        v1.0.6 audit fix: hash the FULL serialized result before
        truncating the displayed payload. v1.0.5 hashed the truncated
        string, so two outputs that diverged past the truncation
        point collided. The displayed `result` is still capped at
        8 KiB to keep the ledger compact, but `result_hash` is now
        the SHA-256 of the original bytes.
        """
        import hashlib
        import json

        result = self.inner.computer.run(language, code)
        # Serialize once, deterministically. Strings stay strings;
        # dicts/lists go through JSON for stable hashing.
        if isinstance(result, str):
            full_serialized = result
        else:
            full_serialized = json.dumps(result, sort_keys=True, default=str)

        # Hash the FULL bytes — never the truncated display version.
        result_hash = (
            "sha256:"
            + hashlib.sha256(full_serialized.encode("utf-8", errors="replace")).hexdigest()
        )

        # Truncate the *display* copy only. Suffix advertises the size
        # so an operator reading the ledger sees how much was dropped.
        if len(full_serialized) > 8192:
            display = (
                full_serialized[:8192]
                + f"…[truncated {len(full_serialized) - 8192} chars; result_hash covers full bytes]"
            )
        else:
            display = full_serialized

        ledger_entry = {
            "tool": f"oi.computer.run.{language}",
            "args": {"code": code},
            "result": display,
            # Pre-computed hash of the original bytes; the snapshot
            # path picks this up rather than re-hashing the truncated
            # `result` field.
            "result_hash": result_hash,
            "side_effect_class": "irreversible",
        }
        self._ledger.append(ledger_entry)
        return result

    # ---- helpers ----

    def _messages_from_inner(self) -> list[dict[str, str]]:
        msgs = getattr(self.inner, "messages", None)
        if msgs is None:
            return []
        out: list[dict[str, str]] = []
        for m in msgs:
            role = m.get("role", "user") if isinstance(m, dict) else getattr(m, "role", "user")
            content = (
                m.get("content", "") if isinstance(m, dict) else getattr(m, "content", "")
            )
            out.append({"role": str(role), "content": str(content)})
        return out


def wrap_interpreter(
    inner: Any,
    *,
    store: str | os.PathLike[str],
    fs_root: str | os.PathLike[str] = ".",
) -> WrappedInterpreter:
    """Wrap an OpenInterpreter object; returns the :class:`WrappedInterpreter`.

    `store` opens (or creates) a ProcessFork store at the given path.
    """
    import processfork

    return WrappedInterpreter(
        inner=inner,
        store=processfork.PfStore.open(str(store)),
        fs_root=Path(fs_root),
    )
