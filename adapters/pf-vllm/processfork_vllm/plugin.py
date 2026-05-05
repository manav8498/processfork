# SPDX-License-Identifier: MIT
"""vLLM plugin shape — Python side of the cache-layer integration.

The Rust side (``pf-cache::CachePager``) defines the trait this
adapter satisfies. Here in Python we provide:

- :class:`VllmCachePager`: pure-Python implementation that talks to a
  vLLM ``LLMEngine`` via its public interface (``pause_workers``,
  ``cache_engine.gpu_cache``, ``add_request``).
- :class:`VllmPlugin`: the actual ``vllm`` plugin entry point. It
  registers HTTP handlers under ``/v1/processfork/{snapshot,fork,
  checkout,merge}``.
- :func:`build_endpoints`: returns a dict of `{path: handler}` for
  testing without a live vLLM server.

The live FFI into vLLM's worker is gated behind the optional ``[vllm]``
dep; in environments without vLLM the pager raises
:class:`NotImplementedError` with a clear pointer to the README.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, Mapping


class _NotImplemented(NotImplementedError):
    """Distinct subclass so callers can match without catching every NIE."""


@dataclass
class VllmCachePager:
    """Python-side ``CachePager``. Talks to a vLLM ``LLMEngine`` instance.

    Phase-9 ships the surface; the GPU-side bytes-pull lives behind
    the ``[vllm]`` optional dep and the worker-pause API. With
    ``vllm`` not installed, every method raises
    :class:`NotImplementedError` and points at the README.
    """

    engine: Any = None
    page_size_tokens: int = 16
    n_layers: int = 0
    n_heads: int = 0
    head_dim: int = 0
    dtype: str = "bf16"

    def pause(self) -> None:
        if self.engine is None:
            return  # in-test no-op; live mode would call engine.pause_workers()
        raise _NotImplemented(
            "live vLLM pause-worker integration lands in v1.0.1; "
            "see adapters/pf-vllm/README.md."
        )

    def resume(self) -> None:
        if self.engine is None:
            return
        raise _NotImplemented(
            "live vLLM resume integration lands in v1.0.1."
        )

    def occupied_pages(self) -> list[int]:
        if self.engine is None:
            return []
        raise _NotImplemented("live page-table inspection lands in v1.0.1.")

    def read_page(self, ix: int) -> tuple[bytes, bytes]:
        raise _NotImplemented(
            f"page read for ix={ix} requires the GPU FFI; lands in v1.0.1."
        )

    def write_page(self, ix: int, k: bytes, v: bytes) -> None:
        raise _NotImplemented(
            f"page write for ix={ix} requires the GPU FFI; lands in v1.0.1."
        )


# ---- HTTP plugin shape ----

@dataclass
class VllmPlugin:
    """vLLM plugin entry. Registered via the ``--plugin processfork``
    CLI flag once the live HTTP wiring lands."""

    pager: VllmCachePager
    store_path: str = "~/.processfork"

    def endpoints(self) -> Mapping[str, Callable[..., Any]]:
        return build_endpoints(self.pager, self.store_path)


def build_endpoints(
    pager: VllmCachePager,
    store_path: str = "~/.processfork",
) -> dict[str, Callable[..., Any]]:
    """Build the four HTTP handlers as plain callables.

    Each handler returns a JSON-serializable dict; the live plugin
    wraps these in vLLM's HTTP framework. Returning a flat dict
    makes the handlers trivially unit-testable.
    """

    def _snapshot(name: str | None = None) -> Mapping[str, Any]:
        return {
            "ok": False,
            "error": "vllm bit-exact snapshot lands in v1.0.1; "
            "see adapters/pf-vllm/README.md.",
        }

    def _fork(cid: str, n: int = 1) -> Mapping[str, Any]:
        return {"ok": False, "error": "see /v1/processfork/snapshot"}

    def _checkout(cid: str) -> Mapping[str, Any]:
        return {"ok": False, "error": "see /v1/processfork/snapshot"}

    def _merge(from_cid: str, into_cid: str) -> Mapping[str, Any]:
        return {"ok": False, "error": "see /v1/processfork/snapshot"}

    return {
        "/v1/processfork/snapshot": _snapshot,
        "/v1/processfork/fork":     _fork,
        "/v1/processfork/checkout": _checkout,
        "/v1/processfork/merge":    _merge,
    }
