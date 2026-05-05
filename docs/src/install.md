# Install

## Pre-built binaries (recommended)

```bash
# Rust CLI:
cargo install processfork

# Python SDK:
pip install processfork

# TypeScript SDK:
npm install @processfork/sdk
```

The CLI binary is a single static binary <15 MB stripped. The Python
wheel and npm package each carry a small Rust cdylib (~2 MB on macOS
arm64).

## Build from source

```bash
git clone https://github.com/manav8498/processfork
cd processfork

# Rust workspace:
cargo build --release -p processfork
./target/release/pf --help

# Python SDK (needs maturin in a venv):
pip install maturin
maturin develop -m crates/pf-py/Cargo.toml --release --features extension-module

# TypeScript SDK (needs @napi-rs/cli):
cd crates/pf-ts
npm install --no-save @napi-rs/cli@2
./node_modules/.bin/napi build --release --platform
```

## Optional integrations

Per-adapter Python packages live under `adapters/<name>/`:

```bash
pip install processfork-claude-code
pip install processfork-langgraph
pip install processfork-openinterpreter
pip install "processfork-vllm[vllm]"          # needs CUDA + vllm ≥ 0.10
pip install "processfork-sglang[sglang]"      # needs CUDA + sglang ≥ 0.5
pip install "processfork-autogen[autogen]"
pip install "processfork-crewai[crewai]"
```

Each adapter's optional extra (`[vllm]`, `[sglang]`, …) installs the
upstream framework alongside; without it, the adapter import works
but the live integration paths return `NotImplementedError` with a
clear pointer.
