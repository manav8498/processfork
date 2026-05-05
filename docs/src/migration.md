# Migrating to v1.0

Per-integration migration recipes. Each adapter ships its own
README with framework-specific code; this page is the index.

## Claude Code

```bash
pip install processfork-claude-code
pf-wrap-claude
# restart Claude Code; /snapshot, /fork, /merge are now available
```

See: [Claude Code adapter](./integrations/claude-code.md).

## LangGraph

```python
- from langgraph.checkpoint.memory import InMemorySaver
- checkpointer = InMemorySaver()
+ from processfork_langgraph import ProcessForkCheckpointer
+ checkpointer = ProcessForkCheckpointer("~/.processfork")

graph = StateGraph(MyState).compile(checkpointer=checkpointer)
```

## OpenInterpreter

```python
- import interpreter
+ import interpreter
+ from processfork_openinterpreter import wrap_interpreter
+ interpreter = wrap_interpreter(interpreter, store="~/.processfork", fs_root=".")

interpreter.snapshot("pre-rm-rf")
interpreter.chat("rm -rf /tmp/foo")
# interpreter.checkout("pre-rm-rf")  if it goes wrong
```

## vLLM

```bash
pip install "processfork-vllm[vllm]"
vllm serve meta-llama/Llama-3-8B \
  --enforce-deterministic \
  --plugin processfork
```

(Live HTTP wiring lands in v1.0.1 per [adapter README](https://github.com/processfork/processfork/tree/main/adapters/pf-vllm).)

## SGLang

```bash
pip install "processfork-sglang[sglang]"
python -m sglang.launch_server \
  --model meta-llama/Llama-3-8B \
  --plugin processfork \
  --deterministic-mode
```

## AutoGen / CrewAI

See per-adapter pages for the runtime + memory drop-ins.

## Operator-side prereqs

The full v1.0 ship list is in [release-checklist](./release-checklist.md);
the registry-creds and GPU-host items are operator-supplied.
