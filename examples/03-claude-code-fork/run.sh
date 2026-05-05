#!/usr/bin/env bash
# examples/03-claude-code-fork — installs the slash-commands and
# demonstrates the recorder snapshotting a synthetic Claude Code
# session into a `.pfimg`.
#
# Usage:    bash examples/03-claude-code-fork/run.sh
# Requires: processfork wheel built and installed in $PYTHON_BIN.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

PYTHON=${PYTHON:-/tmp/_pf_venv2/bin/python}
if [[ ! -x "$PYTHON" ]]; then
    echo "Build the SDK first:"
    echo "  uv venv --python 3.12 /tmp/_pf_venv2"
    echo "  VIRTUAL_ENV=/tmp/_pf_venv2 maturin develop -m crates/pf-py/Cargo.toml --release --features extension-module"
    echo "  PYTHONPATH=adapters/pf-claude-code /tmp/_pf_venv2/bin/python examples/03-claude-code-fork/run.py"
    exit 1
fi

PYTHONPATH=adapters/pf-claude-code "$PYTHON" examples/03-claude-code-fork/run.py
