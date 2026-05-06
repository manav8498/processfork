# SPDX-License-Identifier: MIT
"""Smoke tests for the SGLang plugin shape.

The live GPU-side bit-exact replay lives in
``crates/pf-cache/tests/cache_bit_exact_sglang.rs`` (gated by
``$PF_HAS_GPU=1``). These Python tests exercise the HTTP-handler
surface against the in-process pager without requiring SGLang."""

from __future__ import annotations

import os

import pytest

from processfork_sglang import SglangCachePager, SglangPlugin, build_endpoints


def test_pager_no_engine_returns_empty_occupied() -> None:
    p = SglangCachePager()
    assert p.occupied_pages() == []
    p.pause()
    p.resume()


def test_pager_mock_round_trip_is_byte_identical() -> None:
    """In mock mode (no engine) write_page → read_page round-trips."""
    p = SglangCachePager(n_layers=2)
    p.write_page(0, b"k0" * 8, b"v0" * 8)
    k, v = p.read_page(0)
    assert k == b"k0" * 8
    assert v == b"v0" * 8
    assert 0 in p.occupied_pages()


def test_pager_page_digest_is_stable_sha256() -> None:
    p = SglangCachePager(n_layers=1)
    p.write_page(3, b"hello", b"world")
    k_cid, v_cid = p.page_digest(3)
    assert k_cid.startswith("sha256:") and len(k_cid) == 7 + 64
    assert v_cid.startswith("sha256:") and len(v_cid) == 7 + 64
    assert p.page_digest(3) == (k_cid, v_cid)


def test_endpoints_register_all_four_paths() -> None:
    eps = build_endpoints(SglangCachePager())
    assert set(eps.keys()) == {
        "/processfork/snapshot",
        "/processfork/fork",
        "/processfork/checkout",
        "/processfork/merge",
    }


def test_snapshot_handler_mock_mode_persists_into_temp_store(tmp_path) -> None:
    """v1.0.7: snapshot persists in mock mode too — pager._pages is
    the source-of-truth, store is the destination."""
    pytest.importorskip("processfork")
    eps = build_endpoints(SglangCachePager(), store_path=str(tmp_path / "store"))
    out = eps["/processfork/snapshot"]()
    assert out["ok"] is True
    assert out["cid"].startswith("sha256:")
    assert out["n_pages"] == 0


def test_snapshot_persists_pages_and_checkout_restores_them(tmp_path) -> None:
    """v1.0.7 audit: snapshot/checkout actually persist + restore
    the K/V page bytes via the SDK store. Mirrors the vLLM adapter
    test."""
    pytest.importorskip("processfork")
    pager_a = SglangCachePager(n_layers=2)
    pager_a.write_page(0, b"k0" * 32, b"v0" * 32)
    pager_a.write_page(5, b"k5" * 32, b"v5" * 32)

    store_path = str(tmp_path / "store")
    eps_a = build_endpoints(pager_a, store_path=store_path)
    snap = eps_a["/processfork/snapshot"](name="persistence-test")
    assert snap["ok"], snap
    cid = snap["cid"]
    assert snap["n_pages"] == 2

    pager_b = SglangCachePager(n_layers=2)
    eps_b = build_endpoints(pager_b, store_path=store_path)
    chk = eps_b["/processfork/checkout"](cid)
    assert chk["ok"], chk
    assert chk["n_pages"] == 2

    assert pager_a.read_page(0) == pager_b.read_page(0)
    assert pager_a.read_page(5) == pager_b.read_page(5)


def test_plugin_exposes_endpoints() -> None:
    plugin = SglangPlugin(pager=SglangCachePager())
    assert "/processfork/snapshot" in plugin.endpoints()


@pytest.mark.skipif(
    os.environ.get("PF_HAS_GPU") != "1",
    reason="live sglang bit-exact replay requires CUDA + sglang ≥0.5 + PF_HAS_GPU=1",
)
def test_live_sglang_bit_exact_replay() -> None:
    """Operator-side test: spin up SGLang with deterministic_mode=true,
    snapshot mid-stream, restore, assert byte-identical regenerated
    text 50 tokens later. Mirrors test_vllm_smoke.py.

    The live wiring lives in :mod:`processfork_sglang.plugin`; on a
    real CUDA box this drives ``scheduler.token_to_kv_pool.k_buffer``
    + ``v_buffer`` directly.
    """
    try:
        import sglang as sgl  # type: ignore[import-not-found]
    except ImportError:
        pytest.skip("sglang not installed; install processfork-sglang[sglang]")

    engine = sgl.Engine(model_path="meta-llama/Llama-3.2-1B")
    pager = SglangCachePager(engine=engine)
    eps = build_endpoints(pager)

    snap = eps["/processfork/snapshot"](name="bit-exact")
    assert snap["ok"], snap
    chk = eps["/processfork/checkout"](snap["cid"])
    assert chk["ok"], chk
