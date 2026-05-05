# Integration: CrewAI

CrewAI v0.130+ exposes a `CrewMemory` plug-point. Wire ProcessFork as the
backing store:

```python
from processfork.crewai import ProcessForkMemory
from crewai import Crew

crew = Crew(
    agents=[researcher, writer, editor],
    memory=ProcessForkMemory(store="~/.processfork", crew_id="ml-blogpost"),
    tasks=[...],
)

result = crew.kickoff()

# Time-travel:
crew.memory.checkout("sha256:...pre-edit")
```

`ProcessForkMemory` captures:
- All three agents' message histories into the trace blob.
- Tool-call ledgers for the writer's web-search, file-write, etc.
- The `output_files/` directory each task writes to (world.fs).
