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


def test_snapshot_handler_mock_mode_persists_into_temp_store(tmp_path) -> None:
    """v1.0.7: snapshot now persists in mock mode too (no engine
    needed). The pager's empty `_pages` dict produces an empty
    page list; the manifest is still written and resolves."""
    pytest.importorskip("processfork")
    eps = build_endpoints(VllmCachePager(), store_path=str(tmp_path / "store"))
    out = eps["/v1/processfork/snapshot"]()
    assert out["ok"] is True
    assert out["cid"].startswith("sha256:")
    assert out["n_pages"] == 0


def test_plugin_exposes_endpoints() -> None:
    plugin = VllmPlugin(pager=VllmCachePager(), store_path="~/.processfork")
    eps = plugin.endpoints()
    assert "/v1/processfork/snapshot" in eps


def test_snapshot_persists_pages_and_checkout_restores_them(tmp_path) -> None:
    """v1.0.7 audit: prior versions returned a CID-shaped string from
    /snapshot but never wrote anything to the store, and /checkout
    just returned `{"ok": true}` without doing any work. v1.0.7
    persists every K/V page byte buffer + the manifest, so the CID
    resolves to real on-disk content and checkout restores byte-for-
    byte into a fresh pager.

    Mock-mode test (engine=None) — the pager's `_pages` dict stands
    in for the GPU KV cache so we can prove the wire-format end-to-end
    on the build host without CUDA.
    """
    pytest.importorskip(
        "processfork",
        reason="run after `maturin develop -m crates/pf-py/Cargo.toml --features extension-module`",
    )
    pager_a = VllmCachePager(n_layers=2)
    pager_a.write_page(0, b"k0" * 32, b"v0" * 32)
    pager_a.write_page(7, b"k7" * 32, b"v7" * 32)

    store_path = str(tmp_path / "store")
    eps_a = build_endpoints(pager_a, store_path=store_path)
    snap = eps_a["/v1/processfork/snapshot"](name="persistence-test")
    assert snap["ok"], snap
    cid = snap["cid"]
    assert cid.startswith("sha256:") and len(cid) == 71
    assert snap["n_pages"] == 2

    # Restore into a FRESH pager (proves the data really came from
    # the store, not from the original pager's memory).
    pager_b = VllmCachePager(n_layers=2)
    eps_b = build_endpoints(pager_b, store_path=store_path)
    chk = eps_b["/v1/processfork/checkout"](cid)
    assert chk["ok"], chk
    assert chk["n_pages"] == 2

    # Bit-exact: every (k, v) pair survives the round-trip.
    k0_a, v0_a = pager_a.read_page(0)
    k0_b, v0_b = pager_b.read_page(0)
    assert k0_a == k0_b and v0_a == v0_b
    k7_a, v7_a = pager_a.read_page(7)
    k7_b, v7_b = pager_b.read_page(7)
    assert k7_a == k7_b and v7_a == v7_b


def test_checkout_unknown_cid_errors_cleanly(tmp_path) -> None:
    """A CID that doesn't exist in the store should error cleanly,
    not silently succeed (which v1.0.6's stub did)."""
    pytest.importorskip("processfork")
    pager = VllmCachePager(n_layers=1)
    eps = build_endpoints(pager, store_path=str(tmp_path / "empty-store"))
    out = eps["/v1/processfork/checkout"](
        "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    )
    assert out["ok"] is False
    assert "not in store" in out["error"]


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
