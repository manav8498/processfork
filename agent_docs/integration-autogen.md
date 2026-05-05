# Integration: AutoGen

Microsoft AutoGen 0.4+ uses a `RuntimeContext` per agent group. Wrap:

```python
from processfork.autogen import processfork_runtime
from autogen_agentchat.teams import RoundRobinGroupChat

team = RoundRobinGroupChat([alice, bob], runtime=processfork_runtime("~/.processfork"))
await team.run(task="...")

# Snapshot the entire multi-agent group:
cid = await team.runtime.snapshot("pre-vote")
forks = await team.runtime.fork(cid, n=4)
```

The wrapped runtime:
- Tracks all agent message histories under one trace blob.
- Tracks each agent's tool-effect ledger separately, merged into a single
  effects blob with per-agent partition.
- Snapshots are atomic across all agents in the team.
