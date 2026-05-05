# processfork-langgraph

ProcessFork checkpointer for [LangGraph](https://langchain-ai.github.io/langgraph/).
Full four-layer snapshots at every node boundary, not just LangGraph's
state dict.

## Install

```bash
pip install "processfork-langgraph[langgraph]"
```

## Use

```python
from langgraph.graph import StateGraph
from processfork_langgraph import ProcessForkCheckpointer

graph = StateGraph(MyState).compile(
    checkpointer=ProcessForkCheckpointer(store="~/.processfork"),
)

graph.invoke({"input": "go"}, config={"configurable": {"thread_id": "demo"}})
```

The checkpointer surface mirrors `langgraph.checkpoint.BaseCheckpointSaver`
so existing code works unchanged. Each checkpoint is now a real
ProcessFork image: model + cache + world + effects + trace.

## Forking a thread

```python
from processfork_langgraph import fork_thread

forks = fork_thread(graph, thread_id="demo", n=12, explore="try alternatives")
for cid in forks:
    print(cid)
```

`fork_thread` uses `pf-merge`'s manifest-level fork: each branch points at
the same layer blobs (CoW; no copy) but has a unique fingerprint and a
single `parents = [<source>]` entry.

See `agent_docs/integration-langgraph.md` for the full spec.
