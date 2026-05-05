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
        import processfork

        messages = list(self._messages_from_inner())
        cid = processfork.snapshot_filesystem(
            self.store,
            agent_kind="open-interpreter",
            fs_root=str(self.fs_root),
            env=dict(os.environ),
            messages=messages,
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
        """Forward to `interpreter.computer.run`, recording into the
        wrapper's effect ledger."""
        ledger_entry = {
            "tool": f"oi.computer.run.{language}",
            "args": {"code": code},
            "side_effect_class": "irreversible",
        }
        self._ledger.append(ledger_entry)
        return self.inner.computer.run(language, code)

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
