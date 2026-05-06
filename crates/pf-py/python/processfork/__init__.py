# SPDX-License-Identifier: MIT
"""ProcessFork — fork() for AI agents.

Python SDK wrapping the Rust core. The actual implementation lives in the
``_pf_py`` cdylib built by maturin from ``crates/pf-py``.

Quick start::

    import processfork as pf

    store = pf.PfStore.open("~/.processfork")
    cid = pf.snapshot_filesystem(
        store,
        agent_kind="claude-code",
        fs_root="/tmp/sandbox",
        env={"PWD": "/tmp/sandbox"},
        messages=[{"role": "user", "content": "hi"}],
    )
    print(cid)  # 'sha256:...'

    # Restore on a different host (or the same host, different path):
    pf.checkout_filesystem(store, cid, "/tmp/restored")

    # Three-way merge of two divergent CIDs (auto-discovers ancestor):
    report = pf.merge(store, cid_a, cid_b)
    if report["overall"] == "conflicted":
        for c in report["world_conflicts"]:
            print("CONFLICT", c["path"])
"""

from __future__ import annotations

from ._pf_py import (  # type: ignore[import-not-found]
    PfStore,
    __version__,
    checkout_filesystem,
    digest_of,
    merge,
    put_blob,
    read_blob,
    read_manifest,
    snapshot_filesystem,
)

__all__ = [
    "PfStore",
    "__version__",
    "checkout_filesystem",
    "digest_of",
    "merge",
    "put_blob",
    "read_blob",
    "read_manifest",
    "snapshot_filesystem",
]
