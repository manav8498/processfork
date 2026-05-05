#!/usr/bin/env bash
# examples/06-vllm-bit-exact — bit-exact KV-cache snapshot/restore against
# a live vLLM ≥0.10 server in batch-invariant mode.
#
# Usage:    PF_HAS_GPU=1 bash examples/06-vllm-bit-exact/run.sh
# Requires: CUDA-capable host + vllm + processfork-vllm[vllm].

set -euo pipefail

if [[ "${PF_HAS_GPU:-0}" != "1" ]]; then
    cat <<'EOF'
┌─ ProcessFork example 06: vllm-bit-exact ──────────────────
│ skipped: needs PF_HAS_GPU=1 + a CUDA host + vllm ≥ 0.10.
│
│ This example exists as the v1.0 "operator-runs-it" deliverable per
│ agent_docs/feature-spec.md M5. The wire format and trait surface are
│ stable (see crates/pf-cache/, agent_docs/cache-layer.md). The live
│ FFI shim into vllm.worker.cache_engine lands in v1.0.1.
│
│ When you have a CUDA box:
│   PF_HAS_GPU=1 bash examples/06-vllm-bit-exact/run.sh
│
│ Runs:
│   1. vllm serve meta-llama/Llama-3-8B --enforce-deterministic
│   2. POST /v1/completions for 50 tokens; record logits of token 49
│   3. POST /v1/processfork/snapshot                         → cid
│   4. SIGKILL the worker
│   5. Spawn fresh vllm; POST /v1/processfork/checkout cid
│   6. POST /v1/completions for 100 more tokens
│   7. assert logit-bit-equal at the resumed branch's token 50
└─
EOF
    exit 0
fi

echo "PF_HAS_GPU=1 set, but the vllm live wiring is the v1.0.1 deferred deliverable."
echo "See adapters/pf-vllm/README.md for the design and v1.0.1 milestones."
exit 2
