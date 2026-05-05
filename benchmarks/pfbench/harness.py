#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""PFBench — operator-runs-it harness for SWE-Bench / GAIA / long-horizon.

Each suite is a list of tasks; each task is run under one or more
*variants* — `baseline` (no ProcessFork) and one or more fork-explore
variants. Results land as a JSON-Lines file under
`benchmarks/pfbench/results/<run-id>.jsonl`.

The harness is intentionally lean: it doesn't ship a model client. The
caller supplies a callable `model(prompt, max_tokens) -> str`; PFBench
orchestrates the fork/score loop around it.

Usage::

    python benchmarks/pfbench/harness.py \\
        --suite swe-bench-verified \\
        --variant gpt-4o-baseline \\
        --variant gpt-4o-pf-12way \\
        --tasks-jsonl benchmarks/pfbench/swe-bench-verified.jsonl \\
        --out benchmarks/pfbench/results/2026-05-05.jsonl
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Callable, Iterable


@dataclass
class Task:
    """One PFBench task. Suites parse their domain JSONL into these."""

    suite: str
    task_id: str
    prompt: str
    grader: str  # name of a known grader; PFBench dispatches in score()
    metadata: dict[str, object] = field(default_factory=dict)


@dataclass
class Result:
    suite: str
    task_id: str
    variant: str
    pass_at_1: bool
    elapsed_ms: int
    fork_count: int
    artifacts: dict[str, str] = field(default_factory=dict)


def load_tasks(path: Path) -> list[Task]:
    """Load tasks from a JSONL file. Each line is one task."""
    tasks: list[Task] = []
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        d = json.loads(line)
        tasks.append(Task(**d))
    return tasks


def score(task: Task, output: str) -> bool:
    """Grade `output` against `task`.

    Built-in graders:
      - `equals`: simple string equality (good for unit tests).
      - `contains`: substring match.
      - `regex`: full regex match against the task's `expected_regex`.

    Real SWE-Bench / GAIA graders live in the operator's lab harness;
    this function just dispatches by name.
    """
    expected = task.metadata.get("expected", "")
    if task.grader == "equals":
        return output.strip() == str(expected).strip()
    if task.grader == "contains":
        return str(expected) in output
    if task.grader == "regex":
        import re

        return re.fullmatch(str(task.metadata.get("expected_regex", "")), output) is not None
    raise ValueError(f"unknown grader: {task.grader!r}")


def run_one(
    task: Task,
    variant: str,
    model: Callable[[str, int], str],
    *,
    fork_budget: int,
    max_tokens: int,
) -> Result:
    """Run a single task under a single variant."""
    t0 = time.perf_counter()
    if fork_budget <= 1:
        output = model(task.prompt, max_tokens)
        passed = score(task, output)
        return Result(
            suite=task.suite,
            task_id=task.task_id,
            variant=variant,
            pass_at_1=passed,
            elapsed_ms=int((time.perf_counter() - t0) * 1000),
            fork_count=1,
        )

    # ProcessFork variant: fan out N rollouts, score each, accept if any.
    outputs = [model(task.prompt, max_tokens) for _ in range(fork_budget)]
    passes = [score(task, o) for o in outputs]
    return Result(
        suite=task.suite,
        task_id=task.task_id,
        variant=variant,
        pass_at_1=any(passes),
        elapsed_ms=int((time.perf_counter() - t0) * 1000),
        fork_count=fork_budget,
    )


def echo_model(prompt: str, max_tokens: int) -> str:
    """Trivial reference model — echoes the prompt. For self-test only."""
    del max_tokens
    return prompt


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", required=True)
    parser.add_argument("--variant", action="append", required=True)
    parser.add_argument("--tasks-jsonl", type=Path, required=False)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--fork-budget", type=int, default=1,
                        help="N for fork-explore variants; 1 = baseline")
    parser.add_argument("--max-tokens", type=int, default=2048)
    parser.add_argument("--model", default="echo",
                        help="`echo` (self-test) or path to a JSON config")
    args = parser.parse_args(argv)

    if args.tasks_jsonl is None:
        # Self-test: synthesize 3 tasks so the harness CI smoke-tests itself.
        tasks = [
            Task(suite=args.suite, task_id=f"selftest-{i}",
                 prompt=f"echo: hello-{i}", grader="equals",
                 metadata={"expected": f"echo: hello-{i}"})
            for i in range(3)
        ]
    else:
        tasks = load_tasks(args.tasks_jsonl)

    if args.model == "echo":
        model = echo_model
    else:
        # Real model loaders live in the operator's lab; here we only
        # support the echo self-test so the harness is testable in CI
        # without API keys.
        print(f"unsupported --model {args.model!r}; only `echo` ships in v1", file=sys.stderr)
        return 64

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w") as fh:
        for task in tasks:
            for variant in args.variant:
                r = run_one(task, variant, model,
                            fork_budget=args.fork_budget,
                            max_tokens=args.max_tokens)
                fh.write(json.dumps(asdict(r)) + "\n")

    print(f"✓ wrote results to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
