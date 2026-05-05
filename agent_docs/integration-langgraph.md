# Integration: LangGraph

LangGraph has a `Checkpointer` interface. We provide
`processfork.langgraph.ProcessForkCheckpointer` which captures all four
ProcessFork layers at every checkpoint, not just LangGraph's state dict.

```python
from processfork.langgraph import ProcessForkCheckpointer
from langgraph.graph import StateGraph

graph = StateGraph(MyState).compile(
    checkpointer=ProcessForkCheckpointer(store="~/.processfork"),
)

# Existing LangGraph API works unchanged; the checkpoints are now full
# ProcessFork images: model + cache + world + effects + trace.
config = {"configurable": {"thread_id": "demo"}}
graph.invoke({"input": "go"}, config=config)

# Fork a thread:
from processfork.langgraph import fork_thread
forks = fork_thread(graph, "demo", n=12, explore="try alternatives")
```

`examples/02-twelve-way-parallel/` uses LangGraph + ProcessFork to fan out 12
branches on a math-reasoning task and pick the best.
