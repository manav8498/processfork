# SPDX-License-Identifier: MIT
"""AutoGen runtime backed by a ProcessFork store.

Synchronous in v1 (matches the rest of the SDK). The async surface
expected by AutoGen 0.4 is wrapped via a thin adapter that calls
``asyncio.to_thread`` on each entry point — added in v1.1.
"""

from __future__ import annotations

import os
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Mapping


@dataclass
class _AgentState:
    name: str
    messages: list[Mapping[str, str]] = field(default_factory=list)
    tool_calls: list[Mapping[str, Any]] = field(default_factory=list)


@dataclass
class ProcessForkRuntime:
    """ProcessFork-backed runtime context for an AutoGen agent team.

    Construct via :func:`processfork_runtime`. The runtime tracks each
    agent's chat history + tool calls; ``snapshot`` writes a single
    `.pfimg` covering the whole team atomically.
    """

    store: Any
    fs_root: Path
    agents: dict[str, _AgentState] = field(default_factory=dict)
    snapshots: dict[str, str] = field(default_factory=dict)

    # ---- AutoGen-shaped record API ----

    def register_agent(self, name: str) -> None:
        self.agents.setdefault(name, _AgentState(name=name))

    def record_message(self, agent: str, role: str, content: str) -> None:
        self.register_agent(agent)
        self.agents[agent].messages.append({"role": role, "content": content})

    def record_tool_call(
        self,
        agent: str,
        tool: str,
        args: Mapping[str, Any],
        result: Mapping[str, Any],
        side_effect_class: str = "irreversible",
    ) -> None:
        self.register_agent(agent)
        self.agents[agent].tool_calls.append(
            {"tool": tool, "args": args, "result": result, "side_effect_class": side_effect_class}
        )

    # ---- snapshot / fork ----

    def snapshot(self, name: str) -> str:
        import processfork

        # Flatten every agent's messages into a single trace blob,
        # prefixed by the agent name so the merge engine's summarizer
        # can attribute per-speaker.
        merged: list[dict[str, str]] = []
        for agent_name, state in sorted(self.agents.items()):
            for m in state.messages:
                merged.append({"role": m["role"], "content": f"[{agent_name}] {m['content']}"})

        cid = processfork.snapshot_filesystem(
            self.store,
            agent_kind="autogen-team",
            fs_root=str(self.fs_root),
            env=dict(os.environ),
            messages=merged,
        )
        self.snapshots[name] = cid
        return cid

    def fork(self, cid: str, n: int = 1, *, explore: str | None = None) -> list[str]:
        """Manifest-level fork via the `pf` CLI. Returns the new CIDs."""
        import shutil
        import subprocess

        pf = shutil.which("pf")
        if pf is None:
            raise RuntimeError("`pf` binary not on PATH; build with `cargo build --release -p processfork`")
        store_path = self._store_path()
        cmd = [pf, "--store", str(store_path), "fork", cid, "-n", str(n)]
        if explore:
            cmd += ["--explore", explore]
        out = subprocess.run(cmd, check=True, capture_output=True, text=True)
        return [line.strip() for line in out.stdout.splitlines() if line.strip()]

    def _store_path(self) -> Path:
        repr_ = repr(self.store)
        if "at " in repr_:
            return Path(repr_.split("at ", 1)[1].rstrip(">"))
        return Path.home() / ".processfork"


def processfork_runtime(
    store: str | os.PathLike[str],
    *,
    fs_root: str | os.PathLike[str] | None = None,
) -> ProcessForkRuntime:
    """Open (or create) a runtime against the given store path.

    `fs_root` defaults to a fresh tempdir; pass an existing dir if
    your AutoGen team writes intermediate artefacts you want
    snapshotted into the world layer.
    """
    import processfork

    s = processfork.PfStore.open(str(store))
    root = Path(fs_root) if fs_root else Path(tempfile.mkdtemp(prefix="pf-autogen-"))
    return ProcessForkRuntime(store=s, fs_root=root)
