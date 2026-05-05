# Integration adapters

ProcessFork ships seven first-party adapters covering the major
agent-runtime ecosystems. Each lives as its own pure-Python package
under `adapters/<name>/` so you only install the ones you need.

| Adapter                                 | Status (v1.0)                       |
|-----------------------------------------|-------------------------------------|
| [Claude Code](./claude-code.md)         | ✅ end-to-end on the build host     |
| [LangGraph](./langgraph.md)             | ✅ end-to-end                       |
| [OpenInterpreter](./openinterpreter.md) | ✅ end-to-end                       |
| [vLLM](./vllm.md)                       | trait + 501 stubs; live in v1.0.1   |
| [SGLang](./sglang.md)                   | trait + 501 stubs; live in v1.0.1   |
| [AutoGen](./autogen.md)                 | runtime + tests; CrewAI-shaped      |
| [CrewAI](./crewai.md)                   | memory adapter; round-trip tested   |

The four "scaffolded to v1.0.1" adapters export the trait + URL
parsing + auth-token plumbing today; the live FFI / network paths
return `NotImplementedError` with a clear pointer to the README.
