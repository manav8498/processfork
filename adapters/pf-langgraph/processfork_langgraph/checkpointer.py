# SPDX-License-Identifier: MIT
"""LangGraph checkpointer backed by a ProcessFork store.

Two design choices worth highlighting:

1. **No hard dep on langgraph** at import time. The checkpointer
   class duck-types to the LangGraph protocol; you can use it with or
   without the langgraph package. Tests exercise the duck-typed
   surface without requiring a langgraph install.

2. **State serialization is JSON**. LangGraph's stock checkpointers
   serialize via msgpack / pickle; we use JSON so the resulting
   snapshot is human-debuggable and has zero pickle attack surface.
   Round-trip the (caller-supplied) state through ``json.dumps``
   yourself if your state contains non-JSON values.
"""

from __future__ import annotations

import json
import os
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterator, Mapping


@dataclass
class Checkpoint:
    """A single LangGraph checkpoint persisted as a ProcessFork image."""

    cid: str
    thread_id: str
    state: Mapping[str, Any]


class ProcessForkCheckpointer:
    """Implements the LangGraph ``BaseCheckpointSaver`` protocol.

    Each checkpoint becomes a `.pfimg`:
      - **trace** = the LangGraph state dict, JSON-encoded as a single
        ``{"role":"system","content":"<state json>"}`` message.
      - **world.fs** = the cwd at checkpoint time (or a caller-supplied
        ``fs_root``).

    The checkpoints index lives in-memory; on process restart, walk the
    store with ``pf log`` and re-wire by ``thread_id`` (the fingerprint
    field is set to ``thread:<id>:<step>``).
    """

    def __init__(self, store: str | os.PathLike[str], *, fs_root: str | None = None) -> None:
        import processfork

        self._store = processfork.PfStore.open(str(store))
        self._fs_root = Path(fs_root) if fs_root else None
        # In-memory thread → list[CID] index. Persistent reconstruction
        # is via pf-cli's `log` walk.
        self._threads: dict[str, list[str]] = {}

    # --- BaseCheckpointSaver-shaped surface ---

    def put(self, thread_id: str, state: Mapping[str, Any]) -> Checkpoint:
        """Persist ``state`` for ``thread_id`` and return the new checkpoint."""
        import processfork

        # World-layer FS: caller-supplied or a fresh tempdir we own.
        if self._fs_root is not None:
            fs_root = self._fs_root
        else:
            fs_root = Path(tempfile.mkdtemp(prefix=f"pf-langgraph-{thread_id}-"))
        # Encode the state dict as a single trace message so the merge
        # engine's trace-summarizer has something to look at.
        message = {
            "role": "system",
            "content": "langgraph-state " + json.dumps(state, sort_keys=True, default=str),
        }
        cid = processfork.snapshot_filesystem(
            self._store,
            agent_kind="langgraph",
            fs_root=str(fs_root),
            env={},
            messages=[message],
        )
        self._threads.setdefault(thread_id, []).append(cid)
        return Checkpoint(cid=cid, thread_id=thread_id, state=dict(state))

    def get(self, thread_id: str) -> Checkpoint | None:
        """Return the most recent checkpoint for ``thread_id``."""
        cids = self._threads.get(thread_id)
        if not cids:
            return None
        cid = cids[-1]
        return self._read_checkpoint(thread_id, cid)

    def list(self, thread_id: str) -> Iterator[Checkpoint]:
        """Iterate every checkpoint for ``thread_id``, oldest → newest."""
        for cid in self._threads.get(thread_id, []):
            yield self._read_checkpoint(thread_id, cid)

    def _read_checkpoint(self, thread_id: str, cid: str) -> Checkpoint:
        """Reconstitute the original state dict from the trace blob.

        v1.0.3 audit fix: prior to this, get() returned a placeholder
        ``{"_manifest": ...}`` dict — restored agents got nothing
        useful. We now read the trace blob via the SDK's `read_blob`,
        decode the first message (a JSONL-encoded LangGraph state
        wrapper written by `put`), and return the full state dict.
        """
        import processfork

        manifest = processfork.read_manifest(self._store, cid)
        trace_digest = manifest["trace"]["messages"]
        try:
            trace_bytes = processfork.read_blob(self._store, trace_digest)
        except Exception as e:
            # Defensive: an old v1.0.2 checkpoint with no trace bytes
            # (we never actually wrote the empty trace as a blob lookup)
            # should still surface a sensible Checkpoint.
            return Checkpoint(
                cid=cid,
                thread_id=thread_id,
                state={"_manifest": manifest, "_decode_error": str(e)},
            )

        text = trace_bytes.decode("utf-8", errors="replace").strip()
        if not text:
            return Checkpoint(cid=cid, thread_id=thread_id, state={})
        # The trace is one JSON message per line. We wrote the state as
        # the first message's content with a "langgraph-state " prefix.
        first_line = text.splitlines()[0]
        try:
            msg = json.loads(first_line)
            content = msg.get("content", "")
            if isinstance(content, str) and content.startswith("langgraph-state "):
                state_json = content.removeprefix("langgraph-state ")
                return Checkpoint(
                    cid=cid,
                    thread_id=thread_id,
                    state=json.loads(state_json),
                )
        except (json.JSONDecodeError, ValueError):
            pass
        # Couldn't parse — fall back to the manifest placeholder so the
        # caller still gets a usable Checkpoint object.
        return Checkpoint(
            cid=cid,
            thread_id=thread_id,
            state={"_manifest": manifest, "_raw_trace": text[:512]},
        )


def fork_thread(
    checkpointer: ProcessForkCheckpointer,
    thread_id: str,
    *,
    n: int = 1,
    explore: str | None = None,
) -> list[str]:
    """Fork the latest checkpoint for ``thread_id`` into ``n`` divergent
    branches; returns each new CID.

    Implementation: invokes the `pf` CLI's ``fork`` subcommand against
    the same store. We don't go through the SDK because Phase-7 didn't
    expose ``fork`` as a Python function (it lands in v1.1).
    """
    import shutil
    import subprocess

    cids = checkpointer._threads.get(thread_id, [])
    if not cids:
        raise ValueError(f"no checkpoints recorded for thread {thread_id!r}")
    parent = cids[-1]
    pf = shutil.which("pf") or "pf"
    cmd = [pf, "--store", str(checkpointer._store_root()), "fork", parent, "-n", str(n)]
    if explore:
        cmd += ["--explore", explore]
    out = subprocess.run(cmd, check=True, capture_output=True, text=True)
    return [line.strip() for line in out.stdout.splitlines() if line.strip()]


# Tiny helper: resolve the on-disk store root from the SDK handle.
# v1.1 will expose this directly via PfStore.root().
def _ProcessForkCheckpointer__store_root(self: ProcessForkCheckpointer) -> Path:  # noqa: N807
    repr_ = repr(self._store)  # `<PfStore at /path/to/store>`
    if "at " in repr_:
        return Path(repr_.split("at ", 1)[1].rstrip(">"))
    return Path.home() / ".processfork"


ProcessForkCheckpointer._store_root = _ProcessForkCheckpointer__store_root  # type: ignore[attr-defined]
