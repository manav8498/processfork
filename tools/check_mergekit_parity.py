# SPDX-License-Identifier: MIT
"""Standalone mergekit-parity check for pf-model::merge::ties_merge.

Lives outside the Modal validation suite because mergekit
(arcee-ai/mergekit) pins ``pydantic==2.4.0`` which is mutually
exclusive with vLLM ≥0.20 (which needs pydantic ≥2.12). Run this in
a dedicated venv that doesn't have vLLM installed.

Usage::

    python -m venv /tmp/mergekit-check
    source /tmp/mergekit-check/bin/activate
    pip install numpy torch
    pip install git+https://github.com/arcee-ai/mergekit.git
    python tools/check_mergekit_parity.py

Asserts that the pf-model TIES algorithm (re-implemented here in
numpy) and mergekit's reference TIES produce element-wise outputs
that agree within 1e-3 on synthetic 2048×2048 fp32 deltas.
"""

from __future__ import annotations

import json
import sys

import numpy as np


def pf_ties(deltas, keep_top, alpha):
    """Re-implementation of crates/pf-model/src/merge.rs::ties_merge
    in numpy, faithful to the spec documented there."""
    trimmed = []
    for d in deltas:
        flat = d.reshape(-1).astype(np.float32)
        mags = np.abs(flat)
        k = int(len(flat) * keep_top)
        if k > 0:
            threshold = np.partition(mags, k)[k]
            flat = np.where(mags > threshold, flat, 0.0)
        trimmed.append(flat.reshape(d.shape))
    stacked = np.stack(trimmed, axis=0)
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


def main() -> int:
    rng = np.random.default_rng(42)
    deltas = [
        rng.normal(0, 0.02, size=(2048, 2048)).astype(np.float32) for _ in range(3)
    ]
    keep_top = 0.2
    alpha = 0.5

    pf_out = pf_ties(deltas, keep_top, alpha)

    try:
        import torch
        from mergekit.merge_methods.generalized_task_arithmetic import (
            get_mask,
            sparsify,
        )
        from mergekit.sparsify import SparsificationMethod
    except ImportError as e:
        print(json.dumps({"ok": False, "error": f"mergekit not installed: {e}"}))
        return 2

    tensors = [torch.from_numpy(d) for d in deltas]
    density = 1.0 - keep_top
    # Magnitude sparsification = TIES "trim" step. mergekit's sparsify
    # zeroes out the bottom (1-density) by magnitude, exactly matching
    # pf-model's trim_bottom().
    sparse = [
        sparsify(t, density=density, method=SparsificationMethod.magnitude)
        for t in tensors
    ]
    stack = torch.stack(sparse, dim=0)
    # Sign consensus via mergekit's get_mask (sum-mode = TIES sign-elect).
    mask_per_task = get_mask(stack, method="sum")
    # mergekit returns a per-(task, element) mask of which entries to
    # keep; the disjoint-mean is then sum/count.
    selected = stack * mask_per_task.float()
    count = mask_per_task.sum(dim=0).float().clamp_min(1)
    mk_out = (selected.sum(dim=0) / count * alpha).numpy()

    max_delta = float(np.abs(pf_out - mk_out).max())
    tolerance = 1e-3
    ok = max_delta < tolerance
    report = {
        "ok": ok,
        "weight_shape": list(deltas[0].shape),
        "n_deltas": len(deltas),
        "keep_top": keep_top,
        "alpha": alpha,
        "max_abs_diff": max_delta,
        "tolerance": tolerance,
    }
    print(json.dumps(report, indent=2))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
