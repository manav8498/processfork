# SPDX-License-Identifier: MIT
"""Smoke tests for the SGLang plugin shape."""

from __future__ import annotations

import os

import pytest

from processfork_sglang import SglangCachePager, SglangPlugin, build_endpoints


def test_pager_no_engine_no_op_pause_resume() -> None:
    p = SglangCachePager()
    p.pause()
    p.resume()
    assert p.occupied_pages() == []


def test_pager_read_page_raises_with_pointer() -> None:
    p = SglangCachePager()
    with pytest.raises(NotImplementedError) as ei:
        p.read_page(0)
    assert "v1.0.1" in str(ei.value)


def test_endpoints_register_all_four_paths() -> None:
    eps = build_endpoints(SglangCachePager())
    assert set(eps.keys()) == {
        "/processfork/snapshot",
        "/processfork/fork",
        "/processfork/checkout",
        "/processfork/merge",
    }


def test_plugin_exposes_endpoints() -> None:
    plugin = SglangPlugin(pager=SglangCachePager())
    assert "/processfork/snapshot" in plugin.endpoints()


@pytest.mark.skipif(
    os.environ.get("PF_HAS_GPU") != "1",
    reason="live sglang bit-exact replay requires CUDA + sglang ≥0.5",
)
def test_live_sglang_bit_exact_replay() -> None:
    pytest.fail("PF_HAS_GPU=1 set but live sglang wiring is the v1.0.1 deferred deliverable.")
