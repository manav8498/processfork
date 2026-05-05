# SPDX-License-Identifier: MIT
"""CrewAI memory adapter — every save() becomes a `.pfimg`."""

from __future__ import annotations

import os
import shutil
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Mapping


@dataclass
class ProcessForkMemory:
    """Implements CrewAI's `CrewMemory` protocol against a ProcessFork store.

    Each :meth:`save` produces a snapshot. :meth:`checkout` restores
    the world layer of a previously saved CID.
    """

    store_path: str
    crew_id: str
    fs_root: Path = field(default_factory=lambda: Path(tempfile.mkdtemp(prefix="pf-crewai-")))
    snapshots: list[str] = field(default_factory=list)
    _store: Any = field(init=False, repr=False, default=None)

    def __post_init__(self) -> None:
        import processfork

        self._store = processfork.PfStore.open(str(self.store_path))

    # ---- CrewAI-shaped surface ----

    def save(self, value: Mapping[str, Any], metadata: Mapping[str, Any] | None = None) -> str:
        """Persist `value` (and optional `metadata`) into a new snapshot."""
        import json

        import processfork

        # CrewAI's memory items are typically `{"task": ..., "output": ...,
        # "agent": ...}`. We encode the value+metadata as a single trace
        # message so the merge engine's summarizer can attribute.
        msg = {
            "role": "system",
            "content": "crewai-memory " + json.dumps({
                "value": value,
                "metadata": dict(metadata) if metadata else {},
                "crew_id": self.crew_id,
            }, sort_keys=True, default=str),
        }
        cid = processfork.snapshot_filesystem(
            self._store,
            agent_kind="crewai",
            fs_root=str(self.fs_root),
            env=dict(os.environ),
            messages=[msg],
        )
        self.snapshots.append(cid)
        return cid

    def search(self, query: str, limit: int = 5) -> list[Mapping[str, Any]]:
        """v1 returns the most recent ``limit`` snapshot CIDs.

        Real semantic search lands in v1.1 once we wire an embedding
        index over the trace blobs.
        """
        return [{"cid": cid, "query": query} for cid in self.snapshots[-limit:][::-1]]

    def reset(self) -> None:
        self.snapshots.clear()

    def checkout(self, cid: str, into: str | os.PathLike[str] | None = None) -> Path:
        """Restore the world layer of ``cid`` into a fresh dir (or
        ``into`` if supplied)."""
        import processfork

        # restore_tree refuses to overwrite an existing path. When the
        # caller supplies `into`, move the existing target aside; when
        # we mint our own tempdir, ensure the leaf doesn't pre-exist.
        if into is None:
            parent = Path(tempfile.mkdtemp(prefix="pf-crewai-restore-"))
            target = parent / "x"
        else:
            target = Path(into)
            if target.exists():
                backup = target.with_suffix(target.suffix + ".pf-bak")
                if backup.exists():
                    shutil.rmtree(backup)
                shutil.move(str(target), str(backup))
        processfork.checkout_filesystem(self._store, cid, str(target))
        return target
