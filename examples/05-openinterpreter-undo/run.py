# SPDX-License-Identifier: MIT
"""examples/05-openinterpreter-undo — snapshot before a destructive op,
'mistakenly' delete a file, then `pf checkout` to restore."""

from __future__ import annotations

import shutil
import tempfile
from pathlib import Path

import processfork as pf  # noqa: F401  — verifies the SDK is importable
from processfork_openinterpreter import wrap_interpreter


class FakeInterpreter:
    def __init__(self) -> None:
        self.messages: list[dict[str, str]] = []
        self.computer = self
    def chat(self, prompt: str) -> None:
        self.messages.append({"role": "user", "content": prompt})
    def run(self, language: str, code: str) -> None: ...


def main() -> None:
    sandbox = Path(tempfile.mkdtemp(prefix="oi-sandbox-"))
    (sandbox / "important.txt").write_text("DO NOT DELETE\n")
    (sandbox / "config.toml").write_text("[server]\nport=8080\n")

    print("┌─ ProcessFork example 05: openinterpreter-undo ────────────")
    print(f"│ sandbox: {sandbox}")
    print("│ before  :", sorted(p.name for p in sandbox.iterdir()))

    inner = FakeInterpreter()
    inner.chat("clean up the sandbox")
    w = wrap_interpreter(inner, store=Path(tempfile.mkdtemp(prefix="oi-store-")), fs_root=sandbox)

    safety_point = w.snapshot("pre-cleanup")
    print(f"│ snapshot 'pre-cleanup' = {safety_point}")

    # 'destructive' op
    (sandbox / "important.txt").unlink()
    (sandbox / "config.toml").unlink()
    print("│ after   :", sorted(p.name for p in sandbox.iterdir()))

    # Restore via checkout — into a fresh dir so we can compare
    restored = w.checkout("pre-cleanup", into=Path(tempfile.mkdtemp(prefix="oi-restored-")) / "x")
    print(f"│ restored to: {restored}")
    print("│ after restore :", sorted(p.name for p in restored.iterdir()))
    assert (restored / "important.txt").read_text() == "DO NOT DELETE\n"
    assert (restored / "config.toml").read_text().startswith("[server]")
    print("└─ ✓ snapshot → destructive op → checkout restored byte-identical")


if __name__ == "__main__":
    main()
