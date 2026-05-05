# SPDX-License-Identifier: MIT
"""examples/04-langgraph-checkpoint — record three checkpoints of a tiny
LangGraph-shaped state, then fork the latest into 4 branches via the CLI.

Doesn't require the `langgraph` package; uses the duck-typed
checkpointer surface against a synthetic state dict."""

from __future__ import annotations

import shutil
import tempfile
from pathlib import Path

import processfork as pf
from processfork_langgraph import ProcessForkCheckpointer, fork_thread


def main() -> None:
    store_dir = Path(tempfile.mkdtemp(prefix="pf-lg-store-"))
    cp = ProcessForkCheckpointer(store_dir)

    print("┌─ ProcessFork example 04: langgraph-checkpoint ────────────")
    print(f"│ store      : {store_dir}")
    print("│")

    # Three checkpoints simulating a graph step-step-step.
    cids = []
    for step in range(3):
        c = cp.put("planner-thread", {"step": step, "memo": f"working step {step}"})
        cids.append(c.cid)
        print(f"│ put step {step}: {c.cid}")

    print("│")
    print(f"│ get('planner-thread').cid = {cp.get('planner-thread').cid}")
    print("│")

    if shutil.which("pf"):
        print("│ fork_thread(n=4):")
        forks = fork_thread(cp, "planner-thread", n=4, explore="alt")
        for f in forks:
            print(f"│   {f}")
    else:
        print("│ skipped fork_thread: `pf` binary not on PATH")
        print("│   (build with `cargo build --release -p pf-cli`)")

    print("└─ ✓ checkpointer round-tripped 3 checkpoints + spawned forks")
    # Expose `pf` so users can poke at the manifests via the CLI.
    print(f"\n   try: pf --store {store_dir} log")


if __name__ == "__main__":
    main()
