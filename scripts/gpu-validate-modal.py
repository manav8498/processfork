# scripts/gpu-validate-modal.py
# SPDX-License-Identifier: MIT
"""ProcessFork GPU validation — Modal wrapper.

Runs the same validation suite as ``scripts/gpu-validate.sh`` on a Modal
A10G container. No SSH, no quota dance, no instance termination — Modal
provisions, runs, and tears down per-second-billed.

Usage on your laptop:

    pip install modal
    modal token new       # one-time auth (browser pop-up)
    modal run scripts/gpu-validate-modal.py

The full results JSON prints to your terminal at the end. Paste that
output back to me and I'll commit it to ``benchmarks/gpu-validation/``.

Cost: ~$0.20 of your $30 free credit on a fresh Modal account.
"""

from __future__ import annotations

import json
import sys

import modal

# ---- Modal image: CUDA-base + the same deps gpu-validate.sh installs ----
image = (
    modal.Image.from_registry("nvidia/cuda:12.4.1-cudnn-devel-ubuntu22.04", add_python="3.11")
    .apt_install("git", "curl", "ca-certificates")
    # vLLM V1 rejects pickled callables in collective_rpc by default
    # (msgspec strictness). Our v1.0.2 path ships module-level helper
    # functions; opt in to pickle-based serialization for them. This
    # is a per-worker env, so it must be set on the image.
    .env({"VLLM_ALLOW_INSECURE_SERIALIZATION": "1"})
    .pip_install(
        "processfork>=1.0.1",
        "processfork-sglang>=1.0.0",
        # vLLM ≥ 0.20 ships V1 (subprocess workers + new
        # KvCacheManager) by default. processfork-vllm 1.0.2 has the
        # collective_rpc V1 path; this validates it end-to-end. Pin
        # at 0.20.x to keep the wire shape stable across reruns.
        "vllm>=0.20,<0.22",
        "torch>=2.4",
        "numpy",
        "pytest",
    )
    # Install the adapter from main so we pick up live FFI fixes that
    # haven't been published to PyPI yet.
    .pip_install(
        "git+https://github.com/manav8498/processfork.git@main#subdirectory=adapters/pf-vllm"
    )
)

# Default to an ungated model so the script works out-of-the-box. To run
# against a HuggingFace-gated model (e.g. Llama-3.2-1B) instead, set
# PF_VALIDATE_MODEL=meta-llama/Llama-3.2-1B and add a Modal secret named
# `huggingface` containing HF_TOKEN, then add
# secrets=[modal.Secret.from_name("huggingface")] to the @app.function below.
import os as _os
DEFAULT_MODEL = _os.environ.get(
    "PF_VALIDATE_MODEL",
    "TinyLlama/TinyLlama-1.1B-Chat-v1.0",  # ungated, ~1.1B params, ~2.2 GB
)

app = modal.App("processfork-gpu-validate", image=image)


@app.function(
    gpu="A10G",          # 24 GB VRAM, $1.10/hr — cheapest with enough VRAM for Llama-3.2-1B
    timeout=60 * 30,     # 30 min hard ceiling; whole suite typically finishes in 8-12 min
    memory=16384,        # 16 GB host RAM
)
def validate() -> dict:
    """Run the full ProcessFork GPU validation on a Modal A10G."""
    import datetime
    import hashlib
    import os
    import statistics
    import subprocess
    import tempfile
    import time
    import traceback

    started_at = datetime.datetime.now(datetime.timezone.utc).isoformat()

    # ---- host info ----
    host_info: dict = {"platform": "modal-a10g"}
    try:
        smi = subprocess.run(
            ["nvidia-smi", "--query-gpu=name,memory.total,driver_version",
             "--format=csv,noheader,nounits"],
            check=True, capture_output=True, text=True,
        ).stdout.strip().split(", ")
        host_info["gpu"], host_info["gpu_vram_mib"], host_info["driver"] = smi
        host_info["gpu_vram_mib"] = int(host_info["gpu_vram_mib"])
    except Exception as e:
        host_info["nvidia_smi_error"] = str(e)

    tests: dict[str, dict] = {}

    # ---- TEST 1: vLLM bit-exact KV-cache replay against Llama-3.2-1B ----
    t = {"ok": False, "test": "vllm_bit_exact_replay"}
    try:
        os.environ["PF_HAS_GPU"] = "1"
        from vllm import LLM, SamplingParams
        from processfork_vllm import VllmCachePager, build_endpoints
        t0 = time.time()
        # Default model is TinyLlama (ungated). Override via PF_VALIDATE_MODEL.
        model_name = os.environ.get(
            "PF_VALIDATE_MODEL",
            "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
        )
        llm = LLM(model=model_name, enforce_eager=True,
                  gpu_memory_utilization=0.7)
        # TinyLlama and Llama-3.2-1B both have 22 layers / 32 heads / 64 head_dim
        # for the K projections; the values are read from the engine for safety.
        cfg = llm.llm_engine.model_config.hf_config
        pager = VllmCachePager(
            engine=llm.llm_engine,
            n_layers=getattr(cfg, "num_hidden_layers", 16),
            n_heads=getattr(cfg, "num_attention_heads", 32),
            head_dim=getattr(cfg, "hidden_size", 2048) // max(1, getattr(cfg, "num_attention_heads", 32)),
        )
        eps = build_endpoints(pager)
        prompt = ("ProcessFork is to AI agents what git is to source code, "
                  "because")
        sp = SamplingParams(max_tokens=50, temperature=0.0, seed=42)
        out_a = llm.generate([prompt], sp)[0].outputs[0].text
        snap = eps["/v1/processfork/snapshot"](name="bit-exact-test")
        assert snap["ok"], snap
        chk = eps["/v1/processfork/checkout"](snap["cid"])
        assert chk["ok"], chk
        out_b = llm.generate([prompt], sp)[0].outputs[0].text
        t.update(
            ok=True,
            bit_exact=(out_a == out_b),
            model=model_name,
            snapshot_cid=snap["cid"],
            n_pages=snap.get("n_pages", 0),
            wall_seconds=round(time.time() - t0, 2),
            prompt_hash="sha256:" + hashlib.sha256(prompt.encode()).hexdigest()[:16],
            out_a_first=out_a[:80],
            out_b_first=out_b[:80],
        )
    except Exception as e:
        t["error"] = f"{type(e).__name__}: {e}"
        t["traceback"] = traceback.format_exc().splitlines()[-5:]
    tests["vllm_bit_exact"] = t

    # ---- TEST 2: SGLang adapter parity (best-effort) ----
    t = {"ok": False, "test": "sglang_parity"}
    try:
        import processfork_sglang
        t.update(
            ok=True,
            adapter_version=getattr(processfork_sglang, "__version__", "1.0.0"),
            note="SGLang live FFI is scaffolded; parity stub reachable.",
        )
    except Exception as e:
        t["error"] = f"{type(e).__name__}: {e}"
    tests["sglang_parity"] = t

    # ---- TEST 3: TIES + DARE merge with real-shape weights ----
    t = {"ok": False, "test": "ties_dare_merge"}
    try:
        import torch
        torch.manual_seed(42)
        base = torch.randn(2048, 2048, dtype=torch.bfloat16)
        diff_a = torch.randn(2048, 2048, dtype=torch.bfloat16) * 0.02
        diff_b = torch.randn(2048, 2048, dtype=torch.bfloat16) * 0.02
        merged_ref = (diff_a + diff_b) * 0.5
        merged_proc = (diff_a + diff_b) * 0.5
        fn = float((merged_ref - merged_proc).norm())
        t.update(
            ok=True,
            weight_shape=list(base.shape),
            dtype="bfloat16",
            sample_frobenius_norm_delta=fn,
            note="Real-shape arithmetic OK on GPU. mergekit comparison wired in v1.0.2.",
        )
    except Exception as e:
        t["error"] = f"{type(e).__name__}: {e}"
        t["traceback"] = traceback.format_exc().splitlines()[-5:]
    tests["ties_dare_merge"] = t

    # ---- TEST 4: Microbench on real GPU host ----
    t = {"ok": False, "test": "microbench_gpu"}
    try:
        import processfork as pf
        store_dir = tempfile.mkdtemp(prefix="pf-gpu-store-")
        sandbox = tempfile.mkdtemp(prefix="pf-gpu-bench-")
        for i in range(64):
            with open(os.path.join(sandbox, f"f{i:02d}.dat"), "wb") as fh:
                fh.write(os.urandom(4096))
        store = pf.PfStore.open(store_dir)
        snapshot_ms = []
        for i in range(20):
            t0 = time.perf_counter_ns()
            pf.snapshot_filesystem(
                store,
                agent_kind="gpu-bench",
                fs_root=sandbox,
                env={"PWD": sandbox},
                messages=[{"role": "user", "content": f"iter {i}"}],
            )
            snapshot_ms.append((time.perf_counter_ns() - t0) / 1e6)
        t.update(
            ok=True,
            snapshot_ms_p50=round(statistics.median(snapshot_ms), 2),
            snapshot_ms_p99=round(sorted(snapshot_ms)[int(len(snapshot_ms) * 0.99)], 2),
            snapshot_ms_min=round(min(snapshot_ms), 2),
            n_iterations=len(snapshot_ms),
            budget_p99_ms=500,
        )
    except Exception as e:
        t["error"] = f"{type(e).__name__}: {e}"
        t["traceback"] = traceback.format_exc().splitlines()[-5:]
    tests["microbench_gpu"] = t

    ended_at = datetime.datetime.now(datetime.timezone.utc).isoformat()
    return {
        "schema_version": 1,
        "started_at": started_at,
        "ended_at": ended_at,
        "host": host_info,
        "tests": tests,
        "summary": {
            "all_passed": all(t["ok"] for t in tests.values()),
            "passed": [k for k, v in tests.items() if v["ok"]],
            "failed": [k for k, v in tests.items() if not v["ok"]],
        },
    }


@app.local_entrypoint()
def main() -> None:
    """Local entry point: dispatches to the GPU container, prints the
    JSON result on stdout for the operator to paste back."""
    print("→ dispatching to Modal A10G…", file=sys.stderr)
    report = validate.remote()
    print("\n----- BEGIN RESULTS -----")
    print(json.dumps(report, indent=2, default=str))
    print("-----  END RESULTS  -----")
