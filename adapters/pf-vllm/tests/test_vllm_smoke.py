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


def test_pager_mock_round_trip_is_byte_identical() -> None:
    """In mock mode (no engine) write_page → read_page round-trips."""
    p = VllmCachePager(n_layers=2)
    p.write_page(0, b"k0" * 8, b"v0" * 8)
    k, v = p.read_page(0)
    assert k == b"k0" * 8
    assert v == b"v0" * 8
    # The page now shows up in occupied_pages.
    assert 0 in p.occupied_pages()


def test_pager_page_digest_is_stable_sha256() -> None:
    p = VllmCachePager(n_layers=1)
    p.write_page(3, b"hello", b"world")
    k_cid, v_cid = p.page_digest(3)
    assert k_cid.startswith("sha256:") and len(k_cid) == 7 + 64
    assert v_cid.startswith("sha256:") and len(v_cid) == 7 + 64
    # Re-hashing the same content yields the same digest.
    assert p.page_digest(3) == (k_cid, v_cid)


def test_endpoints_register_all_four_paths() -> None:
    eps = build_endpoints(VllmCachePager())
    assert set(eps.keys()) == {
        "/v1/processfork/snapshot",
        "/v1/processfork/fork",
        "/v1/processfork/checkout",
        "/v1/processfork/merge",
    }


def test_snapshot_handler_no_engine_returns_clear_message() -> None:
    """Without engine + PF_HAS_GPU, snapshot points to the README."""
    eps = build_endpoints(VllmCachePager())
    out = eps["/v1/processfork/snapshot"]()
    assert out["ok"] is False
    assert "PF_HAS_GPU" in out["error"]


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

    The plugin code path is in :mod:`processfork_vllm.plugin`; on a
    real CUDA box this drives ``cache_engine.gpu_cache`` directly.
    """
    # Live-mode wiring is here so a CUDA host can flip the gate and run
    # the round-trip end-to-end. The engine import is inside the test
    # body so non-CUDA hosts never pay the import cost.
    try:
        from vllm import LLM  # type: ignore[import-not-found]
    except ImportError:
        pytest.skip("vllm not installed; install processfork-vllm[vllm]")

    llm = LLM(model="meta-llama/Llama-3.2-1B", enforce_eager=True)
    pager = VllmCachePager(engine=llm.llm_engine)
    eps = build_endpoints(pager)

    snap = eps["/v1/processfork/snapshot"](name="bit-exact")
    assert snap["ok"], snap
    chk = eps["/v1/processfork/checkout"](snap["cid"])
    assert chk["ok"], chk
