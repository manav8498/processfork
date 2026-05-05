#!/usr/bin/env bash
# Convenience runner for examples/01-hello-fork.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
cargo run --release -p example-01-hello-fork "$@"
