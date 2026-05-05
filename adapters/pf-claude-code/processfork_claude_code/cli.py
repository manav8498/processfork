# SPDX-License-Identifier: MIT
"""`pf-wrap-claude` entry point.

Two responsibilities:

1. Drop the slash-command files into the user's Claude Code config.
2. Print a one-line confirmation and the next-step pointer so the user
   knows what to do.
"""

from __future__ import annotations

import argparse
import sys

from .install import install_slash_commands


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="pf-wrap-claude",
        description="Install ProcessFork slash-commands into Claude Code.",
    )
    parser.add_argument(
        "--claude-dir",
        help="Claude Code config dir (default: $HOME/.claude).",
        default=None,
    )
    parser.add_argument(
        "--store",
        help="Where the local ProcessFork store lives (default: $HOME/.processfork).",
        default=None,
    )
    args = parser.parse_args(argv)

    out = install_slash_commands(args.claude_dir)
    print(f"✓ ProcessFork slash-commands installed at {out}")
    print("  Restart Claude Code; /snapshot, /fork, /merge are now available.")
    return 0


if __name__ == "__main__":  # pragma: no cover
    sys.exit(main())
