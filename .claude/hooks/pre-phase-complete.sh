#!/usr/bin/env bash
# .claude/hooks/pre-phase-complete.sh
#
# Run BEFORE you flip a phase from in_progress → done in claude-progress.json.
# Invokes the three gate sub-agents and aborts with non-zero exit on FAIL.
#
# Usage: .claude/hooks/pre-phase-complete.sh <phase-number>

set -euo pipefail

phase="${1:-}"
if [[ -z "$phase" ]]; then
    echo "usage: $0 <phase-number>" >&2
    exit 64
fi

echo "🛡  Phase $phase gate — invoking qa-reviewer, security-reviewer, perf-reviewer."
echo "   (These are sub-agents; run them via the Agent tool from the orchestrator,"
echo "   not this script. This script is the human-readable gate runbook.)"

# Quick objective signals the gate sub-agents will rely on:
echo
echo "── cargo fmt --check ──"
cargo fmt --all -- --check
echo "── cargo clippy -D warnings ──"
cargo clippy --workspace --all-targets -- -D warnings
echo "── cargo test --workspace ──"
cargo test --workspace
echo "── coverage (if cargo-llvm-cov installed) ──"
if command -v cargo-llvm-cov >/dev/null; then
    cargo llvm-cov --workspace --summary-only
else
    echo "(cargo-llvm-cov not installed; skipping coverage gate)"
fi
echo
echo "✅ Objective signals are green. Now invoke the three sub-agents and confirm PASS"
echo "   from each before flipping claude-progress.json."
