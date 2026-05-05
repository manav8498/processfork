# SPDX-License-Identifier: MIT
"""examples/08-rl-rollout-fabric — fan out N rollouts from a shared
synthetic-fixture root, then "score" each by its content-id and keep
the winner.

This example demonstrates the RL-rollout-fabric pattern (snapshot →
fan out N branches → score each → merge the winner) using only the
synthetic fixture so it runs on the build host without any
inference engine."""

from __future__ import annotations

import shutil
import subprocess
import tempfile
from pathlib import Path

import processfork as pf


def main() -> None:
    store_dir = Path(tempfile.mkdtemp(prefix="pf-rl-"))
    sand = Path(tempfile.mkdtemp(prefix="pf-rl-sand-"))
    (sand / "task.md").write_text("# task: maximise the lexicographic CID\n")

    print("┌─ ProcessFork example 08: rl-rollout-fabric ───────────────")
    print(f"│ store: {store_dir}")

    # Root snapshot (the policy "before rollout").
    pf_bin = shutil.which("pf") or "pf"
    root = subprocess.run(
        [pf_bin, "--store", str(store_dir), "snapshot",
         "--agent-id", "rl-policy", "--fs-root", str(sand)],
        check=True, capture_output=True, text=True,
    ).stdout.strip()
    print(f"│ root  : {root}")

    # Fork N rollouts.
    n = 8
    forks = subprocess.run(
        [pf_bin, "--store", str(store_dir), "fork", root,
         "-n", str(n), "--explore", "rollout"],
        check=True, capture_output=True, text=True,
    ).stdout.strip().splitlines()
    print(f"│ forks : {len(forks)}")

    # "Score" by lexicographic CID — the most sortable wins.
    winner = max(forks)
    print(f"│ winner: {winner}")
    print("│")

    # Merge the winner back into the root via the CLI.
    rep = subprocess.run(
        [pf_bin, "--store", str(store_dir), "merge",
         winner, "--into", root],
        check=True, capture_output=True, text=True,
    )
    print("│ merge result:")
    for line in rep.stdout.splitlines():
        print(f"│   {line}")

    print("└─ ✓ N=8 rollouts fanned out, winner merged back via pf-merge")


if __name__ == "__main__":
    main()
