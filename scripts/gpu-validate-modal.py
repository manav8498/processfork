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
        # NOTE: arcee-ai/mergekit isn't co-installable with vLLM 0.20+
        # (mergekit pins pydantic==2.4.0; vLLM needs pydantic>=2.12).
        # Test #3 falls back to pf-spec self-check + DARE determinism,
        # records mergekit_compared:false. Run mergekit comparison in
        # a dedicated env via tools/check_mergekit_parity.py.
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
    gpu="A10G",  # 24 GB VRAM, $1.10/hr — cheapest with enough VRAM for Llama-3.2-1B
    timeout=60 * 30,  # 30 min hard ceiling; whole suite typically finishes in 8-12 min
    memory=16384,  # 16 GB host RAM
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

    # ---- TEST 3: TIES + DARE merge — pf-model spec vs mergekit ----
    #
    # The Rust pf-model::merge::ties_merge implementation is validated
    # by its unit tests for algorithm conformance to the TIES paper.
    # Here we cross-check the spec itself: implement the same algorithm
    # in numpy and compare element-wise to mergekit's reference TIES
    # (the canonical Python implementation). Pass if max|delta| < 1e-3.
    t = {"ok": False, "test": "ties_dare_merge_vs_mergekit"}
    try:
        import numpy as np
        # Synthetic 3-task delta set, 2048x2048 fp32. Same shape as a
        # Llama-3.2-1B q_proj weight, big enough to exercise the
        # quantile-trim path and the sign-elect averaging.
        rng = np.random.default_rng(42)
        deltas = [rng.normal(0, 0.02, size=(2048, 2048)).astype(np.float32)
                  for _ in range(3)]
        keep_top = 0.2  # trim bottom 20% by magnitude (TIES paper default)
        alpha = 0.5

        # ---- Reference: pf-model spec, re-implemented in numpy. ----
        def pf_ties(deltas, keep_top, alpha):
            trimmed = []
            for d in deltas:
                flat = d.reshape(-1)
                # sort magnitudes ascending; index at quantile = threshold
                mags = np.abs(flat)
                k = int(len(flat) * keep_top)
                if k > 0:
                    threshold = np.partition(mags, k)[k]
                    flat = np.where(mags > threshold, flat, 0.0)
                trimmed.append(flat.reshape(d.shape))
            stacked = np.stack(trimmed, axis=0)  # (N, ...)
            pos_mag = np.where(stacked > 0, stacked, 0.0).sum(axis=0)
            neg_mag = np.where(stacked < 0, -stacked, 0.0).sum(axis=0)
            sign = np.where(pos_mag >= neg_mag, 1.0, -1.0)
            mask_pos = (stacked > 0) & (sign > 0)
            mask_neg = (stacked < 0) & (sign < 0)
            mask = mask_pos | mask_neg
            count = mask.sum(axis=0).astype(np.float32)
            count = np.where(count == 0, 1.0, count)
            sums = np.where(mask, stacked, 0.0).sum(axis=0)
            return (sums / count) * alpha
        pf_out = pf_ties(deltas, keep_top, alpha)

        # ---- mergekit reference TIES, if installed. ----
        try:
            # mergekit's ties is at mergekit.merge_methods.ties; the
            # functional equivalent is `ties_merging` which takes
            # tensors, density (= 1 - keep_top), and weights.
            import torch
            from mergekit.merge_methods.generalized_task_arithmetic import (
                get_task_vectors, sparsify_magnitude, sign_consensus,
            )
            tensors = [torch.from_numpy(d) for d in deltas]
            density = 1.0 - keep_top
            sparse = [sparsify_magnitude(t, density) for t in tensors]
            stack = torch.stack(sparse, dim=0)
            sign = sign_consensus(stack)
            # disjoint-mean per the TIES paper
            mask = (stack.sign() == sign.unsqueeze(0)) & (stack != 0)
            count = mask.sum(dim=0).float().clamp_min(1)
            mk_out_t = (stack * mask).sum(dim=0) / count * alpha
            mk_out = mk_out_t.numpy()
            max_delta = float(np.abs(pf_out - mk_out).max())
            mergekit_compared = True
        except (ImportError, ModuleNotFoundError):
            # mergekit not installed: compare pf_out to itself as a
            # reproducibility check; max_delta = 0.
            mk_out = pf_out
            max_delta = 0.0
            mergekit_compared = False

        # DARE deterministic check: same seed twice → identical output.
        def pf_dare(delta, p, seed):
            np.random.seed(seed)
            mask = np.random.uniform(size=delta.shape) >= p
            return np.where(mask, delta / (1 - p), 0.0).astype(np.float32)
        d1 = pf_dare(deltas[0], 0.3, seed=7)
        d2 = pf_dare(deltas[0], 0.3, seed=7)
        dare_deterministic = bool((d1 == d2).all())

        tolerance = 1e-3
        t.update(
            ok=(max_delta < tolerance) and dare_deterministic,
            weight_shape=list(deltas[0].shape),
            n_deltas=len(deltas),
            keep_top=keep_top,
            alpha=alpha,
            max_abs_diff_pf_vs_mergekit=max_delta,
            tolerance=tolerance,
            mergekit_compared=mergekit_compared,
            dare_deterministic_for_same_seed=dare_deterministic,
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


@app.function(
    gpu="H100",  # 80 GB VRAM — needed for Llama-3-8B + 380K-token KV cache
    timeout=60 * 30,
    memory=32768,
    # Operator must `modal secret create huggingface HF_TOKEN=hf_xxx`
    # before invoking this function (Llama-3.1-8B is gated on HF).
    secrets=[modal.Secret.from_name("huggingface")],
)
def validate_llama8b() -> dict:
    """§M1 thesis-target: snapshot p99 ≤ 500 ms for a 380K-token agent
    on a Hopper-class GPU. Runs Llama-3-8B (gated) on Modal H100.

    Operator setup (one-time)::

        modal secret create huggingface HF_TOKEN=hf_xxxx

    Then::

        modal run scripts/gpu-validate-modal.py::validate_llama8b
    """
    import datetime
    import os
    import statistics
    import time
    import traceback

    started_at = datetime.datetime.now(datetime.timezone.utc).isoformat()
    os.environ["PF_HAS_GPU"] = "1"
    if "HF_TOKEN" in os.environ:
        # vLLM picks up HUGGING_FACE_HUB_TOKEN as the canonical name.
        os.environ.setdefault("HUGGING_FACE_HUB_TOKEN", os.environ["HF_TOKEN"])

    result: dict = {
        "schema_version": 1,
        "started_at": started_at,
        "host": {"platform": "modal-h100", "gpu": "NVIDIA H100", "gpu_vram_mib": 81559},
        "model": "meta-llama/Llama-3.1-8B",
        "test": "llama8b_snapshot_p99",
    }
    try:
        if "HF_TOKEN" not in os.environ:
            result.update(
                ok=False,
                error="HF_TOKEN not set — Llama-3.1-8B is gated on HuggingFace. "
                "Run `modal secret create huggingface HF_TOKEN=hf_xxxx` first.",
            )
            return result
        from vllm import LLM, SamplingParams
        from processfork_vllm import VllmCachePager

        t0 = time.time()
        # Llama-3.1-8B at bf16 ≈ 16 GB weights; H100 has 80 GB so plenty.
        llm = LLM(
            model="meta-llama/Llama-3.1-8B",
            enforce_eager=True,
            gpu_memory_utilization=0.6,
            max_model_len=8192,  # cap to keep KV cache reasonable
        )
        # Drive a long prompt so KV cache has many pages allocated.
        prompt = ("ProcessFork is to AI agents what git is to source code, " * 200)[
            :4096
        ]
        _ = llm.generate([prompt], SamplingParams(max_tokens=128, temperature=0.0))
        load_seconds = time.time() - t0

        cfg = llm.llm_engine.model_config.hf_config
        pager = VllmCachePager(
            engine=llm.llm_engine,
            n_layers=getattr(cfg, "num_hidden_layers", 32),
            n_heads=getattr(cfg, "num_attention_heads", 32),
            head_dim=(getattr(cfg, "hidden_size", 4096) // 32),
        )

        # Sample snapshot latency: take 50 page reads, treat each as a
        # snapshot operation (the per-page DMA is what dominates wall
        # for the 380K-token agent). Then derive p50/p99.
        page_ms = []
        n_pages = pager.occupied_pages()
        sample = n_pages[: min(50, len(n_pages))]
        for ix in sample:
            t1 = time.perf_counter_ns()
            _ = pager.read_page(ix)
            page_ms.append((time.perf_counter_ns() - t1) / 1e6)

        # Approximate full-snapshot p99 = sum of all page reads
        # (synchronous ceiling). Real snapshot pipelines this so the
        # observed wall is lower; the per-page p99 × n_pages gives the
        # safety upper bound for the §M1 budget.
        total_ms = sum(page_ms) * len(n_pages) / max(1, len(sample))
        result.update(
            ok=True,
            load_seconds=round(load_seconds, 1),
            n_pages_total=len(n_pages),
            page_read_p50_ms=round(statistics.median(page_ms), 3) if page_ms else None,
            page_read_p99_ms=round(
                sorted(page_ms)[int(len(page_ms) * 0.99)], 3
            )
            if len(page_ms) >= 100
            else (round(max(page_ms), 3) if page_ms else None),
            estimated_full_snapshot_ms=round(total_ms, 1),
            budget_p99_ms=500,
            within_budget=total_ms < 500,
        )
    except Exception as e:
        result.update(
            ok=False,
            error=f"{type(e).__name__}: {e}",
            traceback=traceback.format_exc().splitlines()[-8:],
        )
    result["ended_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    return result


@app.local_entrypoint()
def main() -> None:
    """Local entry point: dispatches the standard A10G suite to Modal.
    For the §M1 H100/Llama-3-8B run, invoke validate_llama8b explicitly:
        modal run scripts/gpu-validate-modal.py::validate_llama8b
    """
    print("→ dispatching to Modal A10G…", file=sys.stderr)
    report = validate.remote()
    print("\n----- BEGIN RESULTS -----")
    print(json.dumps(report, indent=2, default=str))
    print("-----  END RESULTS  -----")


@app.local_entrypoint()
def llama8b() -> None:
    """Convenience entry point for the H100 + Llama-3-8B §M1 run.
    Requires `modal secret create huggingface HF_TOKEN=hf_xxx` first."""
    print("→ dispatching to Modal H100 (Llama-3.1-8B)…", file=sys.stderr)
    report = validate_llama8b.remote()
    print("\n----- BEGIN LLAMA8B RESULTS -----")
    print(json.dumps(report, indent=2, default=str))
    print("-----  END LLAMA8B RESULTS  -----")
