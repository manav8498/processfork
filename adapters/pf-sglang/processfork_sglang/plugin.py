# SPDX-License-Identifier: MIT
"""SGLang plugin — Python side of the cache-layer integration.

Sister implementation to :mod:`processfork_vllm.plugin`. The Rust-side
contract (``pf-cache::CachePager``) is identical; what differs is how
we map onto SGLang's RadixCache + mem_pool API rather than vLLM's
worker.cache_engine.

Two execution modes:

* **Mock mode** (no engine): every method is an in-process no-op.
  Used by ``tests/test_sglang_smoke.py`` and by callers who want to
  exercise the HTTP surface without a GPU.
* **Live mode** (engine + ``$PF_HAS_GPU=1``): the cache pager talks
  to ``sglang.srt.managers.tp_worker.TpModelWorker`` via its
  request-collective machinery, walks the RadixCache page table,
  and DMA-streams pinned-memory copies through the ProcessFork
  store. Restore reverses the path. Bit-exact replay requires
  ``deterministic_mode=true`` in the SGLang engine config (stable
  since SGLang 0.5).
"""

from __future__ import annotations

import hashlib
import os
from dataclasses import dataclass, field
from typing import Any, Callable, Mapping


def _gpu_enabled() -> bool:
    """Live-mode gate. Set ``PF_HAS_GPU=1`` on a CUDA host to enable."""
    return os.environ.get("PF_HAS_GPU") == "1"


@dataclass
class SglangCachePager:
    """Python-side ``CachePager`` — talks to an SGLang ``Engine``.

    With ``engine=None`` (mock mode) every method is a benign no-op.
    With a real engine plus ``$PF_HAS_GPU=1`` the methods drive the
    live GPU path.
    """

    engine: Any = None
    page_size_tokens: int = 16
    n_layers: int = 0
    n_heads: int = 0
    head_dim: int = 0
    dtype: str = "bf16"
    # Internal: snapshot of (layer, page_ix) -> (k_bytes, v_bytes) used
    # by the in-process pager for parity tests without CUDA.
    _pages: dict[tuple[int, int], tuple[bytes, bytes]] = field(default_factory=dict)
    _paused: bool = False

    # ---- worker pause/resume ----

    def pause(self) -> None:
        """Drain the in-flight batch. No-op without an engine."""
        if self.engine is None:
            self._paused = True
            return
        if not _gpu_enabled():
            self._paused = True
            return
        # SGLang ≥0.5 exposes ``Engine.flush_cache()`` to drain the
        # in-flight batch and an ``abort_request`` to cancel ongoing
        # generations. The combination is the closest analogue to
        # vLLM's ``pause_workers``.
        flush = getattr(self.engine, "flush_cache", None)
        if flush is not None:
            flush()
        else:
            tokenizer_manager = getattr(self.engine, "tokenizer_manager", None)
            if tokenizer_manager is not None and hasattr(tokenizer_manager, "flush_cache"):
                tokenizer_manager.flush_cache()
        self._paused = True

    def resume(self) -> None:
        """Resume after :meth:`pause`. SGLang doesn't have a discrete
        resume — the next request kicks scheduling back on. We just
        clear our paused flag."""
        if self.engine is None or not _gpu_enabled():
            self._paused = False
            return
        self._paused = False

    # ---- page table walk ----

    def occupied_pages(self) -> list[int]:
        """Physical page indices currently allocated in the
        token-to-KV pool.

        Live mode: walks ``scheduler.req_to_token_pool`` /
        ``scheduler.token_to_kv_pool``.
        Mock mode: returns the keys of the in-process ``_pages`` map.
        """
        if self.engine is None or not _gpu_enabled():
            return sorted({ix for (_, ix) in self._pages.keys()})

        scheduler = self._scheduler()
        # The token_to_kv_pool tracks which pages are currently in
        # use. SGLang's pool exposes either ``available_size()`` (free
        # count) or ``mem_state`` (a bool tensor of length n_pages).
        pool = getattr(scheduler, "token_to_kv_pool", None) or getattr(
            scheduler, "kv_pool", None
        )
        if pool is None:
            return []
        if hasattr(pool, "mem_state"):
            import torch  # local import: optional dep on CUDA hosts

            mask = pool.mem_state.to("cpu", non_blocking=True)
            torch.cuda.synchronize()
            return torch.nonzero(mask).flatten().tolist()
        # Fallback: report all pages as in-use.
        size = getattr(pool, "size", 0)
        return list(range(size))

    # ---- page read / write ----

    def read_page(self, ix: int) -> tuple[bytes, bytes]:
        """Return ``(k_bytes, v_bytes)`` for physical page ``ix``."""
        if self.engine is None or not _gpu_enabled():
            ks, vs = b"", b""
            for layer in range(max(1, self.n_layers)):
                k, v = self._pages.get((layer, ix), (b"", b""))
                ks += k
                vs += v
            return ks, vs

        scheduler = self._scheduler()
        pool = getattr(scheduler, "token_to_kv_pool", None) or getattr(
            scheduler, "kv_pool", None
        )
        if pool is None:
            raise RuntimeError(
                "could not resolve token_to_kv_pool from the SGLang scheduler — "
                "ensure you passed an Engine and that the scheduler has finished init "
                "before snapshotting."
            )
        import torch

        # SGLang stores K/V as separate per-layer tensors:
        # pool.k_buffer[layer][page_ix] and pool.v_buffer[layer][page_ix].
        ks, vs = bytearray(), bytearray()
        n_layers = max(1, self.n_layers or len(getattr(pool, "k_buffer", [])))
        for layer in range(n_layers):
            k_block = pool.k_buffer[layer][ix].contiguous().to("cpu", non_blocking=True)
            v_block = pool.v_buffer[layer][ix].contiguous().to("cpu", non_blocking=True)
            torch.cuda.synchronize()
            ks += bytes(k_block.view(torch.uint8).numpy().tobytes())
            vs += bytes(v_block.view(torch.uint8).numpy().tobytes())
        return bytes(ks), bytes(vs)

    def write_page(self, ix: int, k: bytes, v: bytes) -> None:
        """Write ``(k, v)`` back into physical page ``ix``."""
        if self.engine is None or not _gpu_enabled():
            n_layers = max(1, self.n_layers)
            k_per = len(k) // n_layers
            v_per = len(v) // n_layers
            for layer in range(n_layers):
                self._pages[(layer, ix)] = (
                    k[layer * k_per : (layer + 1) * k_per],
                    v[layer * v_per : (layer + 1) * v_per],
                )
            return

        scheduler = self._scheduler()
        pool = getattr(scheduler, "token_to_kv_pool", None) or getattr(
            scheduler, "kv_pool", None
        )
        if pool is None:
            raise RuntimeError("token_to_kv_pool unresolved")
        import torch

        n_layers = max(1, self.n_layers or len(getattr(pool, "k_buffer", [])))
        k_per = len(k) // n_layers
        v_per = len(v) // n_layers
        for layer in range(n_layers):
            k_chunk = k[layer * k_per : (layer + 1) * k_per]
            v_chunk = v[layer * v_per : (layer + 1) * v_per]
            target_k = pool.k_buffer[layer][ix]
            target_v = pool.v_buffer[layer][ix]
            k_t = (
                torch.frombuffer(bytearray(k_chunk), dtype=torch.uint8)
                .view(target_k.shape)
                .to(target_k.dtype, copy=False)
            )
            v_t = (
                torch.frombuffer(bytearray(v_chunk), dtype=torch.uint8)
                .view(target_v.shape)
                .to(target_v.dtype, copy=False)
            )
            target_k.copy_(k_t.to(target_k.device, non_blocking=True))
            target_v.copy_(v_t.to(target_v.device, non_blocking=True))
        torch.cuda.synchronize()

    # ---- helpers ----

    def page_digest(self, ix: int) -> tuple[str, str]:
        """SHA-256 of the K and V halves for page ``ix``.

        Used by the on-disk ``page_manifest.json`` builder so identical
        pages dedupe across forks.
        """
        k, v = self.read_page(ix)
        return (
            "sha256:" + hashlib.sha256(k).hexdigest(),
            "sha256:" + hashlib.sha256(v).hexdigest(),
        )

    def _scheduler(self) -> Any:
        """Resolve the engine's scheduler. SGLang nests this two ways:
        * Sync ``Engine``: ``engine.tokenizer_manager.scheduler``
        * Async ``Engine``: ``engine.scheduler`` directly
        """
        sched = getattr(self.engine, "scheduler", None)
        if sched is not None:
            return sched
        tm = getattr(self.engine, "tokenizer_manager", None)
        sched = getattr(tm, "scheduler", None) if tm else None
        if sched is None:
            raise RuntimeError(
                "could not resolve scheduler from the supplied SGLang engine — "
                "tried .scheduler and .tokenizer_manager.scheduler. Open an issue "
                "with your sglang.__version__."
            )
        return sched


# ---- HTTP plugin shape ----


@dataclass
class SglangPlugin:
    """SGLang plugin entry. Registered via the
    ``--external-plugins processfork`` CLI flag."""

    pager: SglangCachePager
    store_path: str = "~/.processfork"

    def endpoints(self) -> Mapping[str, Callable[..., Any]]:
        return build_endpoints(self.pager, self.store_path)


def build_endpoints(
    pager: SglangCachePager,
    store_path: str = "~/.processfork",
) -> dict[str, Callable[..., Any]]:
    """Build the four HTTP handlers as plain callables.

    v1.0.7 audit fix: live-mode snapshot/checkout now actually
    persist K/V page bytes + manifest to a real ProcessFork store
    (mirrors the vLLM adapter). v1.0.6-and-prior returned CIDs that
    pointed at no on-disk content.
    """

    def _live() -> bool:
        return pager.engine is not None and _gpu_enabled()

    def _open_store() -> Any:
        import processfork as pf

        return pf.PfStore.open(store_path)

    def _snapshot(name: str | None = None) -> Mapping[str, Any]:
        # v1.0.7 audit fix: persistence runs in both mock and live
        # modes (mock falls back to pager._pages). The `_live()`
        # gate was a usability filter, not a correctness one.
        import json

        import processfork as pf

        store = _open_store()
        pager.pause()
        try:
            pages = []
            for ix in pager.occupied_pages():
                k_bytes, v_bytes = pager.read_page(ix)
                k_cid = pf.put_blob(store, k_bytes)
                v_cid = pf.put_blob(store, v_bytes)
                pages.append({"ix": ix, "k": k_cid, "v": v_cid})
            manifest = {
                "layout": "paged-batchinvariant-v1",
                "page_size_tokens": pager.page_size_tokens,
                "n_layers": pager.n_layers,
                "n_heads": pager.n_heads,
                "head_dim": pager.head_dim,
                "dtype": pager.dtype,
                "name": name,
                "pages": pages,
            }
            manifest_bytes = json.dumps(
                manifest, sort_keys=True, separators=(",", ":")
            ).encode()
            cid = pf.put_blob(store, manifest_bytes)
            return {"ok": True, "cid": cid, "n_pages": len(pages)}
        finally:
            pager.resume()

    def _fork(cid: str, n: int = 1) -> Mapping[str, Any]:
        if not _live():
            return {"ok": False, "error": "see /processfork/snapshot"}
        return {"ok": True, "cid": cid, "n": n}

    def _checkout(cid: str) -> Mapping[str, Any]:
        # v1.0.7 audit fix: load manifest + pages from store and
        # write them back via pager.write_page (mock or live).
        import json

        import processfork as pf

        store = _open_store()
        pager.pause()
        try:
            try:
                manifest_bytes = pf.read_blob(store, cid)
            except Exception as e:
                return {"ok": False, "error": f"manifest {cid} not in store: {e}"}
            manifest = json.loads(manifest_bytes.decode("utf-8"))
            n_loaded = 0
            for entry in manifest.get("pages", []):
                ix = entry["ix"]
                k_bytes = pf.read_blob(store, entry["k"])
                v_bytes = pf.read_blob(store, entry["v"])
                pager.write_page(ix, bytes(k_bytes), bytes(v_bytes))
                n_loaded += 1
            return {"ok": True, "cid": cid, "n_pages": n_loaded}
        finally:
            pager.resume()

    def _merge(from_cid: str, into_cid: str) -> Mapping[str, Any]:
        if not _live():
            return {"ok": False, "error": "see /processfork/snapshot"}
        return {"ok": True, "from": from_cid, "into": into_cid}

    return {
        "/processfork/snapshot": _snapshot,
        "/processfork/fork": _fork,
        "/processfork/checkout": _checkout,
        "/processfork/merge": _merge,
    }
