# Integration: OpenInterpreter

OpenInterpreter is a Python REPL-style agent. Wrap with:

```python
from processfork.openinterpreter import wrap_interpreter
import interpreter

interpreter = wrap_interpreter(interpreter)

interpreter.snapshot("pre-rm-rf")        # safe-point before dangerous ops
interpreter.chat("rm -rf /tmp/foo")
# … if it goes wrong:
interpreter.checkout("pre-rm-rf")        # full restore incl. FS
```

The wrapper:
- Hooks `interpreter.computer.run` to tap the effect ledger.
- Snapshots include the `interpreter.messages` chat history (trace), the
  Python kernel's globals via dill (world.procs proxy), and the working
  directory (world.fs).
- OpenInterpreter doesn't expose a paged KV cache, so `cache` layer is empty
  (skipped in the manifest). This is acceptable; interpreter sessions are
  re-prefillable cheaply.

`examples/05-openinterpreter-undo/` demonstrates undoing a destructive shell
command via `pf checkout`.
