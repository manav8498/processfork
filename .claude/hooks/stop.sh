#!/usr/bin/env bash
# .claude/hooks/stop.sh
#
# Runs at session-stop. Ensures claude-progress.json and claude-plan.md are
# committed (or at least the dirty diff is visible) so the next session can
# resume from a coherent state.
#
# Hooked from .claude/settings.json (Stop event).

set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || echo .)"

if ! git diff --quiet claude-progress.json claude-plan.md 2>/dev/null; then
    cat <<EOF
⚠  claude-progress.json or claude-plan.md has uncommitted changes.
   The next session will read them as-is, but committing now is safer.

   Suggested:
     git add claude-progress.json claude-plan.md
     git commit -m "chore(state): checkpoint at session boundary"
EOF
fi

# Print a one-line summary the operator can see in their terminal.
phase=$(python3 -c "import json; d=json.load(open('claude-progress.json')); print(d.get('current_phase', '?'))" 2>/dev/null || echo "?")
echo "📍 Session stopped at phase $phase. See claude-plan.md for next-step pointers."
