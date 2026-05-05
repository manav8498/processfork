#!/usr/bin/env bash
# .claude/hooks/post-edit.sh
#
# Runs after every Edit/Write of a tracked file. Format and lint just the
# files touched in the most recent edit so we never commit unformatted code.
#
# Hooked from .claude/settings.json (PostToolUse on Edit + Write).

set -euo pipefail

# The harness passes the changed file path(s) on stdin as one per line.
files=()
while IFS= read -r f; do
    [[ -n "$f" && -f "$f" ]] && files+=("$f")
done

# Fall back to "everything Cargo knows about" if stdin gave us nothing.
if [[ ${#files[@]} -eq 0 ]]; then
    cargo fmt --all >/dev/null 2>&1 || true
    exit 0
fi

for f in "${files[@]}"; do
    case "$f" in
        *.rs)
            rustfmt --edition 2024 "$f" >/dev/null 2>&1 || true
            ;;
        *.py)
            command -v ruff >/dev/null && ruff check --fix --quiet "$f" || true
            command -v ruff >/dev/null && ruff format --quiet "$f"      || true
            ;;
        *.ts|*.tsx|*.js|*.jsx|*.json)
            command -v biome >/dev/null && biome check --write "$f" >/dev/null 2>&1 || true
            ;;
    esac
done
