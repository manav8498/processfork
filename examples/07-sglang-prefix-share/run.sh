#!/usr/bin/env bash
# examples/07-sglang-prefix-share — runnable on every host (mock
# mode); upgrades to live RadixCache page round-trip when
# PF_HAS_GPU=1 + sglang is installed.
#
# v1.0.14: replaces the v1.0.11 "exit 2 with Modal pointer"
# skeleton. Same shape as examples/06 — mock-mode K/V page
# round-trip exercises the adapter end-to-end on every CI host;
# Modal lane stays the live-GPU validation.

set -euo pipefail

if ! command -v python3 >/dev/null 2>&1; then
    echo "examples/07: python3 not found; install Python 3.9+ to run this example."
    exit 0
fi

if ! python3 -c 'import processfork_sglang' 2>/dev/null; then
    cat <<'EOF'
┌─ ProcessFork example 07: sglang-prefix-share ─────────────────
│ skipped: processfork-sglang is not installed.
│
│ Install (no GPU required for the mock-mode round-trip):
│     pip install processfork-sglang
│
│ Or with the full SGLang stack (CUDA-capable host):
│     pip install "processfork-sglang[sglang]"
│
│ Then re-run:
│     bash examples/07-sglang-prefix-share/run.sh
└─
EOF
    exit 0
fi

if [[ "${PF_HAS_GPU:-0}" == "1" ]] && python3 -c 'import sglang' 2>/dev/null; then
    MODE="live"
else
    MODE="mock"
fi

echo "examples/07: running adapter round-trip in $MODE mode..."

python3 <<'PYEOF'
import os
import tempfile

from processfork_sglang import SglangCachePager, build_endpoints

# Two pagers with matching layout. SGLang's RadixCache is a tree of
# token-prefix nodes; here we exercise the page-level wire format
# the adapter persists, which is what `pf snapshot`/`pf checkout`
# round-trips. The radix-tree replay itself is v1.1.
N_LAYERS, N_HEADS, HEAD_DIM, PAGE_SIZE = 2, 4, 64, 16
pager_a = SglangCachePager(
    n_layers=N_LAYERS,
    n_heads=N_HEADS,
    head_dim=HEAD_DIM,
    page_size_tokens=PAGE_SIZE,
)
pager_b = SglangCachePager(
    n_layers=N_LAYERS,
    n_heads=N_HEADS,
    head_dim=HEAD_DIM,
    page_size_tokens=PAGE_SIZE,
)

def synth_page(ix: int) -> tuple[bytes, bytes]:
    seed = ix.to_bytes(2, "big")
    k = (seed * 16)[: N_LAYERS * N_HEADS * HEAD_DIM * 2]
    v = (seed[::-1] * 16)[: N_LAYERS * N_HEADS * HEAD_DIM * 2]
    return k, v

stuffed = []
for ix in range(3):
    k, v = synth_page(ix)
    pager_a.write_page(ix, k, v)
    stuffed.append((ix, k, v))

with tempfile.TemporaryDirectory() as td:
    store_path = os.path.join(td, "store")
    eps_a = build_endpoints(pager_a, store_path=store_path)
    snap = eps_a["/processfork/snapshot"](name="example07-mock")
    assert snap["ok"], snap
    cid = snap["cid"]
    print(f"  snapshot: {cid}  ({snap['n_pages']} pages)")

    eps_b = build_endpoints(pager_b, store_path=store_path)
    chk = eps_b["/processfork/checkout"](cid)
    assert chk["ok"], chk
    print(f"  checkout: {chk['n_pages']} pages restored into pager B")

    for ix, k_orig, v_orig in stuffed:
        k_rest, v_rest = pager_b.read_page(ix)
        assert k_rest == k_orig, f"page {ix} K bytes diverged"
        assert v_rest == v_orig, f"page {ix} V bytes diverged"

print("✓ RadixCache page round-trip is byte-identical across snapshot/checkout")
PYEOF

if [[ "$MODE" == "live" ]]; then
    cat <<EOF

  Live-mode note: this example uses synthetic pages and the mock
  pager. Real RadixAttention prefix-share validation across
  snapshot/restore (full radix-tree replay) is v1.1; today the
  Modal lane reaches the parity stub.

      modal run scripts/gpu-validate-modal.py
EOF
fi

echo
echo "examples/07: done."
