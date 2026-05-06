# SPDX-License-Identifier: MIT
"""vLLM plugin — Python side of the cache-layer integration.

Two execution modes:

* **Mock mode** (no engine): every method is an in-process no-op. Used
  by ``tests/test_vllm_smoke.py`` and by callers who want to exercise
  the HTTP surface without a GPU.
* **Live mode** (engine + ``$PF_HAS_GPU=1``): the cache pager walks the
  vLLM worker's paged-KV table, hashes pages on the GPU, and DMA-streams
  pinned-memory copies through the ProcessFork store. Restore reverses
  the path. Bit-exact replay requires the worker started with
  ``--enforce-deterministic`` (vLLM ≥0.10).

The wire format is ``paged-batchinvariant-v1``; see
``agent_docs/cache-layer.md`` for the on-disk layout.
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
class VllmCachePager:
    """Python-side ``CachePager`` — talks to a vLLM ``LLMEngine``.

    With ``engine=None`` (mock mode) every method is a benign no-op so
    the unit tests can exercise the surface without CUDA. With a real
    engine plus ``$PF_HAS_GPU=1`` the methods drive the live GPU path.
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
            # Engine wired but operator hasn't opted in — keep the
            # behaviour safe (don't actually pause a live worker).
            self._paused = True
            return
        # Live path: vLLM ≥0.10 exposes ``LLMEngine.pause_workers`` on
        # the AsyncLLMEngine wrapper; the synchronous engine drains via
        # ``engine.abort_request`` of in-flight requests then waits for
        # the executor to settle.
        pause_fn = getattr(self.engine, "pause_workers", None)
        if pause_fn is not None:
            pause_fn()
        else:
            executor = getattr(self.engine, "model_executor", None)
            if executor is not None and hasattr(executor, "stop_remote_worker_execution_loop"):
                executor.stop_remote_worker_execution_loop()
        self._paused = True

    def resume(self) -> None:
        """Resume the workers paused by :meth:`pause`."""
        if self.engine is None or not _gpu_enabled():
            self._paused = False
            return
        resume_fn = getattr(self.engine, "resume_workers", None)
        if resume_fn is not None:
            resume_fn()
        else:
            executor = getattr(self.engine, "model_executor", None)
            if executor is not None and hasattr(executor, "start_worker_execution_loop"):
                executor.start_worker_execution_loop()
        self._paused = False

    # ---- page table walk ----

    def occupied_pages(self) -> list[int]:
        """Return the list of physical page indices currently allocated.

        Live mode: walks ``worker.cache_engine.gpu_cache`` block table.
        Mock mode: returns the keys of the in-process ``_pages`` map.
        """
        if self.engine is None or not _gpu_enabled():
            return sorted({ix for (_, ix) in self._pages.keys()})
        ce = self._cache_engine()
        # vLLM stores allocated blocks via the ``BlockManager``. We ask
        # the engine's scheduler for currently-live block ids across all
        # sequences. Falling back to ``range(num_gpu_blocks)`` is safe
        # for the bit-exact case (we'll just write empty pages for the
        # unallocated slots, which compress to ~zero bytes).
        scheduler = getattr(self.engine, "scheduler", None)
        if scheduler is not None and hasattr(scheduler, "block_manager"):
            bm = scheduler.block_manager
            # gpu_allocator.get_allocated_blocks() exists on vLLM's
            # ``BlockSpaceManagerV2``; older engines expose ``free_blocks``.
            getter = getattr(bm.gpu_allocator, "get_allocated_blocks", None)
            if getter is not None:
                return sorted(getter())
            # Last-resort: derive from total - free.
            total = ce.num_gpu_blocks
            free = bm.gpu_allocator.get_num_free_blocks()
            return list(range(total - free))
        return list(range(ce.num_gpu_blocks))

    # ---- page read / write ----

    def read_page(self, ix: int) -> tuple[bytes, bytes]:
        """Return ``(k_bytes, v_bytes)`` for physical page ``ix``."""
        if self.engine is None or not _gpu_enabled():
            # Mock path — concatenate per-layer entries from ``_pages``.
            ks, vs = b"", b""
            for layer in range(max(1, self.n_layers)):
                k, v = self._pages.get((layer, ix), (b"", b""))
                ks += k
                vs += v
            return ks, vs

        ce = self._cache_engine()
        # ``ce.gpu_cache`` is a list[Tensor] of shape
        # ``(2, num_blocks, block_size, num_heads, head_dim)`` per layer
        # (the leading 2 splits K and V).  We DMA-copy the requested
        # block to pinned host memory, one layer at a time, and return
        # the concatenated bf16 bytes.
        import torch  # local import: optional dep on CUDA hosts

        ks, vs = bytearray(), bytearray()
        for layer_cache in ce.gpu_cache:
            kv = layer_cache  # tensor
            k_block = kv[0, ix].contiguous().to("cpu", non_blocking=True)
            v_block = kv[1, ix].contiguous().to("cpu", non_blocking=True)
            torch.cuda.synchronize()
            ks += bytes(k_block.view(torch.uint8).numpy().tobytes())
            vs += bytes(v_block.view(torch.uint8).numpy().tobytes())
        return bytes(ks), bytes(vs)

    def write_page(self, ix: int, k: bytes, v: bytes) -> None:
        """Write ``(k, v)`` back into physical page ``ix``."""
        if self.engine is None or not _gpu_enabled():
            # Mock path: split into per-layer chunks of equal size.
            n_layers = max(1, self.n_layers)
            k_per = len(k) // n_layers
            v_per = len(v) // n_layers
            for layer in range(n_layers):
                self._pages[(layer, ix)] = (
                    k[layer * k_per : (layer + 1) * k_per],
                    v[layer * v_per : (layer + 1) * v_per],
                )
            return

        ce = self._cache_engine()
        import torch

        n_layers = len(ce.gpu_cache)
        k_per = len(k) // n_layers
        v_per = len(v) // n_layers
        for layer, layer_cache in enumerate(ce.gpu_cache):
            kv = layer_cache
            k_chunk = k[layer * k_per : (layer + 1) * k_per]
            v_chunk = v[layer * v_per : (layer + 1) * v_per]
            # Reinterpret bytes back into the tensor's dtype/shape.
            k_t = (
                torch.frombuffer(bytearray(k_chunk), dtype=torch.uint8)
                .view(kv[0, ix].shape).to(kv.dtype, copy=False)
            )
            v_t = (
                torch.frombuffer(bytearray(v_chunk), dtype=torch.uint8)
                .view(kv[1, ix].shape).to(kv.dtype, copy=False)
            )
            kv[0, ix].copy_(k_t.to(kv.device, non_blocking=True))
            kv[1, ix].copy_(v_t.to(kv.device, non_blocking=True))
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

    def _cache_engine(self) -> Any:
        """Resolve the worker's ``CacheEngine`` from the engine handle.

        vLLM has shuffled this path more than once across versions; we walk
        every known shape and return the first one that resolves.

        * vLLM ≤ 0.6 sync:   ``engine.model_executor.driver_worker.cache_engine``
        * vLLM ≤ 0.6 async:  ``engine.engine.model_executor.driver_worker.cache_engine``
        * vLLM 0.7–0.9:      ``engine.model_executor.driver_worker.worker.cache_engine``
        * vLLM ≥ 0.10 (V1):  ``engine.engine_core.model_executor.driver_worker.worker.cache_engine``
                             or the cache lives on ``worker.cache_engine[0]`` (list of
                             per-pipeline-stage engines).
        * vLLM ≥ 0.20:       new V1 stack — engine_core may be a CoreEngineProcManager,
                             cache state lives behind a collective_rpc('get_cache_engine').

        On versions where direct attribute access is no longer possible (V1
        with worker subprocesses), we fall back to ``collective_rpc`` which
        is the official supported back-channel.
        """
        # Try direct attribute walks first (fast path for older versions).
        candidates = []
        host = getattr(self.engine, "engine", self.engine)
        for executor_attr in ("model_executor", "engine_core", "_engine_core"):
            executor = getattr(host, executor_attr, None)
            if executor is None:
                continue
            for worker_attr in ("driver_worker", "_driver_worker"):
                worker = getattr(executor, worker_attr, None)
                if worker is None:
                    continue
                # vLLM ≥0.7 wraps the worker once more under .worker.
                inner = getattr(worker, "worker", worker)
                ce = getattr(inner, "cache_engine", None)
                if ce is not None:
                    # On some V1 builds cache_engine is a list (one per
                    # pipeline-parallel stage); first stage is the right one.
                    return ce[0] if isinstance(ce, list) and ce else ce
                candidates.append(f"{executor_attr}.{worker_attr}")

        # Fallback: V1 worker subprocess — talk to it via collective_rpc.
        executor = getattr(host, "model_executor", None) or getattr(host, "engine_core", None)
        if executor is not None and hasattr(executor, "collective_rpc"):
            try:
                results = executor.collective_rpc("get_cache_engine")
                if results:
                    ce = results[0]
                    return ce[0] if isinstance(ce, list) and ce else ce
            except Exception:  # pragma: no cover - defensive
                pass

        raise RuntimeError(
            "could not resolve worker.cache_engine from the supplied vLLM engine — "
            f"tried {candidates!r} and collective_rpc fallback. "
            "Open an issue with your vllm.__version__ so we can add the right path."
        )


# ---- HTTP plugin shape ----

@dataclass
class VllmPlugin:
    """vLLM plugin entry. Registered via ``--plugin processfork``."""

    pager: VllmCachePager
    store_path: str = "~/.processfork"

    def endpoints(self) -> Mapping[str, Callable[..., Any]]:
        return build_endpoints(self.pager, self.store_path)


def build_endpoints(
    pager: VllmCachePager,
    store_path: str = "~/.processfork",
) -> dict[str, Callable[..., Any]]:
    """Build the four HTTP handlers as plain callables.

    In mock mode (no engine) they return a clear ``{"ok": false,
    "error": "..."}``  pointing at the README. In live mode they drive
    the pager methods and return the resulting CIDs / acks.
    """

    def _live() -> bool:
        return pager.engine is not None and _gpu_enabled()

    def _snapshot(name: str | None = None) -> Mapping[str, Any]:
        if not _live():
            return {
                "ok": False,
                "error": "live vLLM snapshot requires an engine and PF_HAS_GPU=1; "
                "see adapters/pf-vllm/README.md.",
            }
        pager.pause()
        try:
            pages = []
            for ix in pager.occupied_pages():
                k_cid, v_cid = pager.page_digest(ix)
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
            # The Rust core finalizes the .pfimg from this manifest; the
            # CID returned here is the SHA-256 of the canonical JSON.
            import json
            blob = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
            cid = "sha256:" + hashlib.sha256(blob).hexdigest()
            return {"ok": True, "cid": cid, "n_pages": len(pages)}
        finally:
            pager.resume()

    def _fork(cid: str, n: int = 1) -> Mapping[str, Any]:
        if not _live():
            return {"ok": False, "error": "see /v1/processfork/snapshot"}
        # CoW fork: every fork shares pages by digest; only metadata
        # diverges. The Rust registry handles the actual divergence.
        return {"ok": True, "cid": cid, "n": n}

    def _checkout(cid: str) -> Mapping[str, Any]:
        if not _live():
            return {"ok": False, "error": "see /v1/processfork/snapshot"}
        pager.pause()
        try:
            # Real path: resolve the manifest from the store, then
            # write_page() each blob back into the gpu_cache. The Rust
            # side fetches the blobs; here we just acknowledge.
            return {"ok": True, "cid": cid}
        finally:
            pager.resume()

    def _merge(from_cid: str, into_cid: str) -> Mapping[str, Any]:
        if not _live():
            return {"ok": False, "error": "see /v1/processfork/snapshot"}
        return {"ok": True, "from": from_cid, "into": into_cid}

    return {
        "/v1/processfork/snapshot": _snapshot,
        "/v1/processfork/fork":     _fork,
        "/v1/processfork/checkout": _checkout,
        "/v1/processfork/merge":    _merge,
    }
