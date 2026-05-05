# processfork-autogen

ProcessFork integration for [AutoGen](https://github.com/microsoft/autogen)
0.4+. Wraps a `RuntimeContext` so a whole agent group's state can be
snapshotted and forked atomically.

## Install

```bash
pip install "processfork-autogen[autogen]"
```

## Use

```python
from processfork_autogen import processfork_runtime
from autogen_agentchat.teams import RoundRobinGroupChat

team = RoundRobinGroupChat(
    [alice, bob],
    runtime=processfork_runtime("~/.processfork"),
)
await team.run(task="...")

# Snapshot the whole team:
cid = await team.runtime.snapshot("pre-vote")
forks = await team.runtime.fork(cid, n=4)
```

The runtime tracks each agent's message history under one trace blob
plus per-agent partitions in the effects ledger. Snapshots are atomic
across the team (no agent's state can advance mid-snapshot).

See `agent_docs/integration-autogen.md` for the spec.
