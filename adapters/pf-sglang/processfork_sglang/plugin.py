# SPDX-License-Identifier: MIT
"""SGLang plugin shape — Python side of the cache-layer integration.

Sister implementation to :mod:`processfork_vllm.plugin`. The Rust-side
contract (``pf-cache::CachePager``) is identical; what differs is how
we map onto the engine's page-table API.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Mapping


class _NotImplemented(NotImplementedError):
    pass


@dataclass
class SglangCachePager:
    """Python-side ``CachePager`` for an SGLang engine.

    Reads/writes proxy through the engine's ``mem_pool`` and
    ``RadixCache`` once the live shim is wired in v1.0.1.
    """

    engine: Any = None
    page_size_tokens: int = 16
    n_layers: int = 0
    n_heads: int = 0
    head_dim: int = 0
    dtype: str = "bf16"

    def pause(self) -> None:
        if self.engine is None:
            return
        raise _NotImplemented(
            "live sglang pause integration lands in v1.0.1; "
            "see adapters/pf-sglang/README.md."
        )

    def resume(self) -> None:
        if self.engine is None:
            return
        raise _NotImplemented("live sglang resume lands in v1.0.1.")

    def occupied_pages(self) -> list[int]:
        if self.engine is None:
            return []
        raise _NotImplemented("live RadixCache walk lands in v1.0.1.")

    def read_page(self, ix: int) -> tuple[bytes, bytes]:
        raise _NotImplemented(
            f"page read for ix={ix} requires the GPU FFI; lands in v1.0.1."
        )

    def write_page(self, ix: int, k: bytes, v: bytes) -> None:
        raise _NotImplemented(
            f"page write for ix={ix} requires the GPU FFI; lands in v1.0.1."
        )


@dataclass
class SglangPlugin:
    pager: SglangCachePager
    store_path: str = "~/.processfork"

    def endpoints(self) -> Mapping[str, Callable[..., Any]]:
        return build_endpoints(self.pager, self.store_path)


def build_endpoints(
    pager: SglangCachePager,
    store_path: str = "~/.processfork",
) -> dict[str, Callable[..., Any]]:
    def _stub(*_a: Any, **_kw: Any) -> Mapping[str, Any]:
        return {
            "ok": False,
            "error": "sglang bit-exact snapshot lands in v1.0.1; "
            "see adapters/pf-sglang/README.md.",
        }

    return {
        "/processfork/snapshot": _stub,
        "/processfork/fork":     _stub,
        "/processfork/checkout": _stub,
        "/processfork/merge":    _stub,
    }
