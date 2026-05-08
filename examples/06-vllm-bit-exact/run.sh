#!/usr/bin/env bash
# examples/06-vllm-bit-exact — bit-exact KV-cache snapshot/restore against
# a live vLLM ≥0.10 server in batch-invariant mode.
#
# Usage:    PF_HAS_GPU=1 bash examples/06-vllm-bit-exact/run.sh
# Requires: CUDA-capable host + vllm + processfork-vllm[vllm].

set -euo pipefail

if [[ "${PF_HAS_GPU:-0}" != "1" ]]; then
    cat <<'EOF'
┌─ ProcessFork example 06: vllm-bit-exact (v1.0.x: Modal lane) ─
│ This example is a SKELETON. It is intentionally not a self-
│ contained "spawn vllm + run pf snapshot + assert bit-exact"
│ flow on your local box.
│
│ The actual vLLM bit-exact validation runs on Modal:
│   modal run scripts/gpu-validate-modal.py
│
│ Latest results (vLLM V0 engine + TinyLlama-1.1B + Modal A10G):
│   bit_exact: true, 38 619 KV pages, byte-identical output
│   benchmarks/gpu-validation/2026-05-06-modal-a10g.json
│
│ V1 engine (collective_rpc): output-equivalent (first-80-chars
│ match) but bit_exact: false. Treat live V1 KV restore as
│ "lossy semantic restore" today — see README "What does and
│ doesn't ship in v1.0.x".
│
│ For interactive use of the vLLM adapter today, install
│ processfork-vllm[vllm] and call pf.snapshot/checkout from
│ inside your engine process — that's the supported path.
└─
EOF
    exit 0
fi

cat <<'EOF'
PF_HAS_GPU=1 was set, but examples/06 has never been a local self-
contained validation. The bit-exact validation IS the Modal lane:

    modal run scripts/gpu-validate-modal.py

(See README.md "Status" → bit-exact rows for the latest result and
the JSON it lands in.)

For interactive use of the vLLM adapter on your CUDA box:
    pip install "processfork-vllm[vllm]"
    # then use pf.snapshot / pf.checkout from inside your vLLM process
EOF
exit 2
