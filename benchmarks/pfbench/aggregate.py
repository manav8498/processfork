#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Aggregate PFBench result JSONLs into a per-(suite, variant) pass@1
summary table. Prints to stdout in markdown."""
from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("results", type=Path, nargs="+",
                        help="One or more results.jsonl files.")
    args = parser.parse_args(argv)

    bucket: dict[tuple[str, str], list[bool]] = defaultdict(list)
    for path in args.results:
        for line in path.read_text().splitlines():
            if not line.strip():
                continue
            r = json.loads(line)
            bucket[(r["suite"], r["variant"])].append(bool(r["pass_at_1"]))

    print("| suite | variant | pass@1 | n |")
    print("|-------|---------|--------|---|")
    for (suite, variant), passes in sorted(bucket.items()):
        n = len(passes)
        rate = sum(passes) / n if n else 0.0
        print(f"| {suite} | {variant} | {rate:.1%} | {n} |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
