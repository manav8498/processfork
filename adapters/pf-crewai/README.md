# processfork-crewai

ProcessFork memory adapter for [CrewAI](https://www.crewai.com/) ≥0.130.
Replaces CrewAI's stock `CrewMemory` with a ProcessFork-backed
implementation; every crew step becomes a `.pfimg` you can time-travel
through.

## Install

```bash
pip install "processfork-crewai[crewai]"
```

## Use

```python
from crewai import Crew
from processfork_crewai import ProcessForkMemory

crew = Crew(
    agents=[researcher, writer, editor],
    memory=ProcessForkMemory(store="~/.processfork", crew_id="ml-blogpost"),
    tasks=[...],
)

result = crew.kickoff()

# Time-travel:
crew.memory.checkout("sha256:...pre-edit")
```

Captures:
- All three agents' message histories into the trace blob.
- Tool-call ledgers (web-search, file-write, etc.).
- The `output_files/` directory each task writes to (world.fs).

See `agent_docs/integration-crewai.md` for the spec.
