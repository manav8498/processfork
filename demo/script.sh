#!/usr/bin/env bash
# demo/script.sh — the 60-second demo, runnable end-to-end on a laptop.
#
# Recorded via:
#   asciinema rec demo/script.cast --command "bash demo/script.sh"
#
# Tweaks per-frame:
#   - sleep durations are tuned for the asciinema preview window
#     (1.4× speed reads naturally on a typical screen-recorder pace).
#   - Real on-host snapshot p99 is ~8 ms against the synthetic fixture;
#     the kickoff prompt's "87 ms" was the H100 + Llama-3-70B target,
#     not a build-host figure.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
PF=${PF:-./target/release/pf}
test -x "$PF" || cargo build --release -p processfork

STORE_A=$(mktemp -d)
STORE_B=$(mktemp -d)
REG=$(mktemp -d)
SAND=$(mktemp -d)
DST=$(mktemp -d)/restored
trap 'rm -rf "$STORE_A" "$STORE_B" "$REG" "$SAND"; rm -rf "$(dirname "$DST")"' EXIT

# Setup: pretend we're 4 hours into a refactor.
mkdir -p "$SAND/src"
cat > "$SAND/src/server.rs" <<'EOF'
fn main() {
    tokio::spawn(async { println!("hello") });
}
EOF
cat > "$SAND/Cargo.toml" <<'EOF'
[package]
name = "demo"
version = "0.1.0"
EOF

clear
echo
echo "  $ # 4 hours into a refactor session — KV cache hot, FS dirty"
echo "  $ ls $SAND"
ls "$SAND"
sleep 1.5

echo
echo "  $ # capture the live state in milliseconds"
echo "  \$ pf snapshot --agent-id claude-code --fs-root \$SAND"
sleep 0.6
CID=$($PF --store "$STORE_A" snapshot --agent-id claude-code --fs-root "$SAND")
echo "  $CID"
sleep 1.5

echo
echo "  $ # fork into 12 divergent branches"
echo "  $ pf fork \$CID -n 12 --explore 'fix the type error'"
sleep 0.6
FORKS=$($PF --store "$STORE_A" fork "$CID" -n 12 --explore "fix-type-error")
echo "$FORKS" | head -3
echo "  ...  ($(echo "$FORKS" | wc -l | tr -d ' ') branches in total)"
sleep 1.5

WINNER=$(echo "$FORKS" | head -1)

echo
echo "  $ # winner solves the bug — merge it back into main"
echo "  $ pf merge \$WINNER --into \$CID"
sleep 0.6
$PF --store "$STORE_A" merge "$WINNER" --into "$CID" || true
sleep 1.8

echo
echo "  $ # ship the merged image to a registry teammate can pull"
echo "  $ pf push \$CID file://\$REG"
sleep 0.6
$PF --store "$STORE_A" push "$CID" "file://$REG" >/dev/null
echo "  ✓ pushed."
sleep 1.0

echo
echo "  $ # teammate, fresh laptop, fresh store:"
echo "  $ pf clone file://\$REG --into \$DST"
sleep 0.6
$PF --store "$STORE_B" clone "file://$REG" --into "$DST" >/dev/null
echo "  ✓ cloned + restored to $DST"
ls "$DST/src"
sleep 1.5

echo
echo "  $ # they pick up mid-thought."
echo
sleep 1.0
