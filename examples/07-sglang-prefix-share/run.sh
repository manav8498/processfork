#!/usr/bin/env bash
# examples/07-sglang-prefix-share — RadixAttention prefix-sharing
# preserved across snapshot/restore.
#
# Usage:    PF_HAS_GPU=1 bash examples/07-sglang-prefix-share/run.sh
# Requires: CUDA host + sglang + processfork-sglang[sglang].

set -euo pipefail

if [[ "${PF_HAS_GPU:-0}" != "1" ]]; then
    cat <<'EOF'
┌─ ProcessFork example 07: sglang-prefix-share (v1.0.x: Modal lane) ─
│ This example is a SKELETON. It does not run a self-contained
│ SGLang prefix-share validation on your local box.
│
│ The validation runs on Modal:
│   modal run scripts/gpu-validate-modal.py
│
│ Latest result: SGLang live FFI is scaffolded; Modal lane reaches
│ the parity stub. Full radix-tree replay across snapshot/restore
│ is v1.1 — see README "What does and doesn't ship in v1.0.x".
│
│ For interactive use of the SGLang adapter today, install
│ processfork-sglang[sglang] and call the SDK from inside your
│ engine process. The mock-mode page round-trip is regression-
│ tested in tests/.
└─
EOF
    exit 0
fi

cat <<'EOF'
PF_HAS_GPU=1 was set, but examples/07 has never been a local self-
contained validation. Use the Modal lane:

    modal run scripts/gpu-validate-modal.py

For interactive use of the SGLang adapter on your CUDA box:
    pip install "processfork-sglang[sglang]"
EOF
exit 2
