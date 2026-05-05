#!/usr/bin/env bash
# examples/07-sglang-prefix-share — RadixAttention prefix-sharing
# preserved across snapshot/restore.
#
# Usage:    PF_HAS_GPU=1 bash examples/07-sglang-prefix-share/run.sh
# Requires: CUDA host + sglang + processfork-sglang[sglang].

set -euo pipefail

if [[ "${PF_HAS_GPU:-0}" != "1" ]]; then
    cat <<'EOF'
┌─ ProcessFork example 07: sglang-prefix-share ─────────────
│ skipped: needs PF_HAS_GPU=1 + a CUDA host + sglang ≥ 0.5.
│
│ Demonstrates that snapshot/restore preserves the SGLang
│ RadixAttention prefix tree — two requests sharing a 1000-token
│ system prompt continue to share their cache pages after restore.
│
│ Runs (when PF_HAS_GPU=1):
│   1. python -m sglang.launch_server --model Llama-3-8B --deterministic-mode
│   2. fire 2 requests sharing a 1000-token prefix; verify prefix is shared
│   3. POST /processfork/snapshot                            → cid
│   4. kill server
│   5. fresh server; POST /processfork/checkout cid
│   6. fire same 2 requests; verify prefix is STILL shared (page table
│      reconstructed)
└─
EOF
    exit 0
fi

echo "PF_HAS_GPU=1 set, but the sglang live wiring is the v1.0.1 deferred deliverable."
echo "See adapters/pf-sglang/README.md."
exit 2
