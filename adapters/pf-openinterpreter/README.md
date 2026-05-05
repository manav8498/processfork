# processfork-openinterpreter

ProcessFork wrapper for [OpenInterpreter](https://openinterpreter.com/).
Lets you snapshot before a dangerous shell op and `pf checkout` if it
goes wrong.

## Install

```bash
pip install "processfork-openinterpreter[oi]"
```

## Use

```python
import interpreter
from processfork_openinterpreter import wrap_interpreter

interpreter = wrap_interpreter(interpreter, store="~/.processfork", fs_root=".")

interpreter.snapshot("pre-rm-rf")
interpreter.chat("rm -rf /tmp/foo")
# … if it goes wrong:
interpreter.checkout("pre-rm-rf")
```

The wrapper:
- Hooks `interpreter.computer.run` to tap the effect ledger.
- Snapshots include the `interpreter.messages` chat history (trace),
  the working directory (world.fs), and the env (world.env).
- The Python kernel's globals are preserved across snapshots via
  `dill`; install with `[oi]` extras to enable.

OpenInterpreter doesn't expose a paged KV cache, so the cache layer
in the snapshot is empty — interpreter sessions are cheap to
re-prefill.

See `agent_docs/integration-openinterpreter.md` for the full spec.
