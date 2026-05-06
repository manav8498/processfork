# SPDX-License-Identifier: MIT
"""Type stubs for the `_pf_py` cdylib.

Hand-written; keep in sync with `crates/pf-py/src/lib.rs`.
"""

from __future__ import annotations

from typing import Any, Mapping, Sequence, TypedDict

__version__: str

class WorldConflict(TypedDict):
    path: str
    a_digest: str
    b_digest: str
    x_digest: str | None

class MergeReport(TypedDict):
    merged_cid: str
    ancestor: str
    overall: str  # "clean" | "conflicted" | "skipped"
    world_conflicts: list[WorldConflict]
    trace_summary: str
    model_applied_task_arithmetic: bool

class PfStore:
    """Local content-addressed `.pfimg` store."""

    @staticmethod
    def open(path: str) -> "PfStore":
        """Open (or create) a ProcessFork store rooted at `path`.

        `~` expansion is honoured.
        """
        ...

    def physical_bytes(self) -> int:
        """Return the total compressed bytes on disk."""
        ...

def digest_of(bytes: bytes) -> str:
    """Compute the SHA-256 digest of `bytes`, formatted `sha256:<hex>`."""
    ...

class _Message(TypedDict):
    role: str
    content: str

class _EffectEntry(TypedDict, total=False):
    """One ACRFence-shaped tool-call ledger entry. All keys optional —
    the SDK coerces missing values, but adapters typically supply at
    least ``tool_id``, ``args_hash``, ``side_effect_class``, and
    ``idempotency_key``."""
    ix: int
    tool_id: str
    args_hash: str
    result_hash: str
    idempotency_key: str
    side_effect_class: str  # "pure" | "idempotent" | "irreversible" | "network-only"

def snapshot_filesystem(
    store: PfStore,
    agent_kind: str,
    fs_root: str,
    env: Mapping[str, str],
    messages: Sequence[_Message],
    effects: Sequence[_EffectEntry] | None = None,
) -> str:
    """Capture FS sandbox + env + chat trace into a `.pfimg`. Returns CID.

    ``effects`` (optional) folds tool-call ledger entries into the
    ``effects.ledger.v1`` blob so a restored agent sees prior side
    effects as facts (ACRFence). Adapters maintain this list as the
    agent runs; empty/None gives a header-only ledger.
    """
    ...

def checkout_filesystem(store: PfStore, cid: str, target_path: str) -> None:
    """Restore the world-layer FS tree of `cid` into `target_path`.

    `target_path` must NOT already exist.
    """
    ...

def read_manifest(store: PfStore, cid: str) -> dict[str, Any]:
    """Load the manifest at `cid` as a Python dict (canonical JSON form)."""
    ...

def read_blob(store: PfStore, digest: str) -> bytes:
    """Fetch the raw bytes of a blob by digest. Used by adapters that
    need to read individual layer blobs (e.g. the LangGraph
    checkpointer reading the trace blob to reconstitute state)."""
    ...

def merge(
    store: PfStore,
    a: str,
    b: str,
    alpha: float | None = None,
    dare_p: float | None = None,
    seed: int | None = None,
) -> MergeReport:
    """Three-way merge B into A; auto-discovers the common ancestor."""
    ...
