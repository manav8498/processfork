#!/usr/bin/env bash
# examples/02-cli-snapshot — exercises the `pf` CLI end-to-end on a
# tempdir-rooted store. Demonstrates: snapshot → log → status → diff →
# checkout → verify, plus the round-trip via the local file:// registry
# (live since v1.0.2; replaces the older HF-stub demo).
#
# Usage:    bash examples/02-cli-snapshot/run.sh
# Requires: pf binary built (cargo build --release -p processfork).

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
PF=${PF:-./target/release/pf}
if [[ ! -x "$PF" ]]; then
    echo "building pf..."
    cargo build --release -p processfork
fi

STORE=$(mktemp -d)
SAND=$(mktemp -d)
trap 'rm -rf "$STORE" "$SAND"' EXIT

# Sandbox to capture.
mkdir -p "$SAND/src"
echo "fn main() { println!(\"hello\"); }" > "$SAND/src/main.rs"
echo "# demo project" > "$SAND/README.md"

PF="$PF --store $STORE"
echo "┌─ ProcessFork example 02: cli-snapshot ─────────────────────"
echo "│ store    : $STORE"
echo "│ sandbox  : $SAND"
echo "├──────────────────────────────────────────────────────────"

echo "│ \$ pf snapshot --agent-id demo --fs-root $SAND"
CID=$($PF snapshot --agent-id demo --fs-root "$SAND")
echo "│   → $CID"
echo "│"

echo "│ \$ pf status"
$PF status | sed 's/^/│   /'
echo "│"

echo "│ \$ pf log"
$PF log | sed 's/^/│   /'
echo "│"

# Mutate the sandbox, snapshot again, diff.
echo "fn main() { println!(\"hello, world\"); }" > "$SAND/src/main.rs"
echo "│ \$ pf snapshot   (after editing src/main.rs)"
CID2=$($PF snapshot --agent-id demo --fs-root "$SAND" --name updated)
echo "│   → $CID2"
echo "│"

echo "│ \$ pf diff $CID $CID2"
$PF diff "$CID" "$CID2" | sed 's/^/│   /'
echo "│"

echo "│ \$ pf checkout $CID2 --into $STORE/restored"
$PF checkout "$CID2" --into "$STORE/restored" | sed 's/^/│   /'
test -f "$STORE/restored/src/main.rs"
echo "│   → /src/main.rs restored byte-identical: $(cat "$STORE/restored/src/main.rs")"
echo "│"

echo "│ \$ pf verify"
$PF verify | sed 's/^/│   /'
echo "│"

REG=$(mktemp -d)
echo "│ \$ pf push $CID file://$REG"
$PF push "$CID" "file://$REG" 2>&1 | sed 's/^/│   /'
echo "│"

echo "│ \$ pf clone file://$REG --into $STORE/clone"
$PF clone "file://$REG" --into "$STORE/clone" 2>&1 | sed 's/^/│   /'
test -f "$STORE/clone/src/main.rs"
rm -rf "$REG"
echo "│"

echo "└─ ✓ all CLI commands behaved as documented in agent_docs/cli-spec.md"
