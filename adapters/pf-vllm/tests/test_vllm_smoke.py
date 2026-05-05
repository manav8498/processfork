# SPDX-License-Identifier: MIT
"""Smoke tests for the vLLM plugin shape.

The live GPU-side bit-exact replay lives in
``crates/pf-cache/tests/cache_bit_exact_vllm.rs`` (gated by
``$PF_HAS_GPU=1``). These Python tests exercise the HTTP-handler
surface against the in-process pager without requiring vLLM."""

from __future__ import annotations

import os

import pytest

from processfork_vllm import VllmCachePager, VllmPlugin, build_endpoints


def test_pager_no_engine_returns_empty_occupied() -> None:
    p = VllmCachePager()
    assert p.occupied_pages() == []
    p.pause()  # no-op without engine
    p.resume()


def test_pager_read_page_raises_with_pointer() -> None:
    p = VllmCachePager()
    with pytest.raises(NotImplementedError) as ei:
        p.read_page(0)
    assert "v1.0.1" in str(ei.value)


def test_endpoints_register_all_four_paths() -> None:
    eps = build_endpoints(VllmCachePager())
    assert set(eps.keys()) == {
        "/v1/processfork/snapshot",
        "/v1/processfork/fork",
        "/v1/processfork/checkout",
        "/v1/processfork/merge",
    }


def test_snapshot_handler_returns_clear_v101_message() -> None:
    eps = build_endpoints(VllmCachePager())
    out = eps["/v1/processfork/snapshot"]()
    assert out["ok"] is False
    assert "v1.0.1" in out["error"]


def test_plugin_exposes_endpoints() -> None:
    plugin = VllmPlugin(pager=VllmCachePager(), store_path="~/.processfork")
    eps = plugin.endpoints()
    assert "/v1/processfork/snapshot" in eps


@pytest.mark.skipif(
    os.environ.get("PF_HAS_GPU") != "1",
    reason="live vLLM bit-exact replay requires CUDA + vllm ≥0.10 + PF_HAS_GPU=1",
)
def test_live_vllm_bit_exact_replay() -> None:
    """Operator-side test: spin up vLLM with --enforce-deterministic,
    snapshot mid-stream, restore, assert logit-identical 100 tokens
    later. Mirrors crates/pf-cache/tests/cache_bit_exact_vllm.rs.

    Lives here too so operators running pytest on a CUDA box hit the
    same gate from both languages.
    """
    pytest.fail(
        "PF_HAS_GPU=1 set but the live vllm wiring is the v1.0.1 deferred "
        "deliverable; install processfork-vllm[vllm] and the matching vllm "
        "version, then re-run."
    )
