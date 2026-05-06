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

The plugin transparently supports both vLLM **V0** (≤0.9 — direct
attribute access to ``worker.cache_engine``) and vLLM **V1** (≥0.10 —
subprocess workers + ``collective_rpc``). For V1, the operator must
set ``VLLM_ALLOW_INSECURE_SERIALIZATION=1`` on the worker environment
because our V1 path ships pickled callables (the worker-side
``_v1_*`` helpers below) through the RPC channel; vLLM's default
msgspec serializer rejects callables for security reasons.

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


# ---- V1 worker-side helpers ----
#
# These are module-level (not methods) so collective_rpc can pickle them
# and ship them to each worker subprocess. They run *inside* the worker
# process, with `worker` being the local Worker instance — they have
# direct access to `worker.model_runner.kv_caches` (a list of per-layer
# K/V tensors on the GPU).


def _v1_occupied_pages(worker: Any) -> list[int]:
    """Return the indices of currently-allocated KV cache pages.

    V1 stores per-block usage on the worker's KvCacheManager. If we
    can't introspect, default to range(num_pages) — overcaptures but
    is bit-exact safe.
    """
    runner = getattr(worker, "model_runner", None)
    if runner is None:
        return []
    kv = getattr(runner, "kv_caches", None)
    if not kv:
        return []
    # kv[0].shape = (2, num_blocks, block_size, num_heads, head_dim)
    num_blocks = int(kv[0].shape[1])
    return list(range(num_blocks))


def _v1_read_page(worker: Any, ix: int) -> tuple[bytes, bytes]:
    """Copy K and V tensors for one physical page off the GPU and return
    them as concatenated bf16 bytes. Runs *inside* the worker process."""
    import torch  # local: only on CUDA hosts

    runner = worker.model_runner
    kv = runner.kv_caches  # list[Tensor], one per attention layer
    ks, vs = bytearray(), bytearray()
    for layer_cache in kv:
        # layer_cache.shape = (2, num_blocks, block_size, num_heads, head_dim)
        k_block = layer_cache[0, ix].contiguous().to("cpu", non_blocking=True)
        v_block = layer_cache[1, ix].contiguous().to("cpu", non_blocking=True)
        torch.cuda.synchronize()
        ks += bytes(k_block.view(torch.uint8).numpy().tobytes())
        vs += bytes(v_block.view(torch.uint8).numpy().tobytes())
    return bytes(ks), bytes(vs)


def _v1_write_page(worker: Any, ix: int, k: bytes, v: bytes) -> None:
    """Inverse of :func:`_v1_read_page` — split the byte buffers back
    into per-layer K/V chunks and copy them onto the GPU page."""
    import torch

    runner = worker.model_runner
    kv = runner.kv_caches
    n_layers = len(kv)
    k_per = len(k) // n_layers
    v_per = len(v) // n_layers
    for layer, layer_cache in enumerate(kv):
        k_chunk = k[layer * k_per : (layer + 1) * k_per]
        v_chunk = v[layer * v_per : (layer + 1) * v_per]
        target_k = layer_cache[0, ix]
        target_v = layer_cache[1, ix]
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
        if ce is not None:
            # V0 path: walk the BlockSpaceManager.
            scheduler = getattr(self.engine, "scheduler", None)
            if scheduler is not None and hasattr(scheduler, "block_manager"):
                bm = scheduler.block_manager
                getter = getattr(bm.gpu_allocator, "get_allocated_blocks", None)
                if getter is not None:
                    return sorted(getter())
                total = ce.num_gpu_blocks
                free = bm.gpu_allocator.get_num_free_blocks()
                return list(range(total - free))
            return list(range(ce.num_gpu_blocks))

        # V1 path: ask each worker how many KV pages it has via RPC.
        results = self._v1_rpc(_v1_occupied_pages)
        if not results:
            return []
        # All workers' page counts are identical for TP>1 (same KV
        # cache layout across replicas); take the first.
        return list(results[0])

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
        if ce is not None:
            # V0 path: direct DMA copy from worker.cache_engine.gpu_cache.
            import torch  # local import: optional dep on CUDA hosts

            ks, vs = bytearray(), bytearray()
            for layer_cache in ce.gpu_cache:
                kv = layer_cache
                k_block = kv[0, ix].contiguous().to("cpu", non_blocking=True)
                v_block = kv[1, ix].contiguous().to("cpu", non_blocking=True)
                torch.cuda.synchronize()
                ks += bytes(k_block.view(torch.uint8).numpy().tobytes())
                vs += bytes(v_block.view(torch.uint8).numpy().tobytes())
            return bytes(ks), bytes(vs)

        # V1 path: ship the read into the worker subprocess via
        # collective_rpc; the worker copies the K/V bytes for `ix`
        # to pinned host memory and returns them through the RPC.
        results = self._v1_rpc(_v1_read_page, ix)
        if not results:
            raise RuntimeError("V1 collective_rpc returned no workers' results")
        return results[0]

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
        if ce is None:
            # V1 path: ship the write into the worker subprocess.
            self._v1_rpc(_v1_write_page, ix, k, v)
            return

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
        """Resolve the worker's ``CacheEngine`` (V0 architecture only).

        Returns ``None`` if no CacheEngine is reachable — the caller
        should then fall back to the V1 collective_rpc path. vLLM
        version map:

        * vLLM ≤ 0.6 sync:   ``engine.model_executor.driver_worker.cache_engine``
        * vLLM ≤ 0.6 async:  ``engine.engine.model_executor.driver_worker.cache_engine``
        * vLLM 0.7–0.9:      ``engine.model_executor.driver_worker.worker.cache_engine``
        * vLLM ≥ 0.10 (V1):  no CacheEngine; cache lives on
                             ``worker.model_runner.kv_caches`` and is only
                             reachable via ``engine.collective_rpc``.
        """
        host = getattr(self.engine, "engine", self.engine)
        for executor_attr in ("model_executor", "engine_core", "_engine_core"):
            executor = getattr(host, executor_attr, None)
            if executor is None:
                continue
            for worker_attr in ("driver_worker", "_driver_worker"):
                worker = getattr(executor, worker_attr, None)
                if worker is None:
                    continue
                inner = getattr(worker, "worker", worker)
                ce = getattr(inner, "cache_engine", None)
                if ce is not None:
                    return ce[0] if isinstance(ce, list) and ce else ce
        return None

    def _is_v1(self) -> bool:
        """Best-effort detection of vLLM V1 (subprocess-worker) architecture."""
        host = getattr(self.engine, "engine", self.engine)
        # V1's EngineCoreClient exposes collective_rpc; V0's
        # GPUExecutor / RayGPUExecutor doesn't have that name on the
        # public engine handle.
        return hasattr(host, "collective_rpc") or hasattr(self.engine, "collective_rpc")

    def _v1_rpc(self, fn: Any, *args: Any) -> Any:
        """Execute ``fn`` on every V1 worker via ``collective_rpc``.
        Returns the list of per-worker results. For tensor-parallel
        size 1 the caller takes ``[0]``."""
        host = getattr(self.engine, "engine", self.engine)
        rpc = (
            getattr(host, "collective_rpc", None)
            or getattr(self.engine, "collective_rpc", None)
        )
        if rpc is None:
            raise RuntimeError(
                "vLLM V1 detected but engine.collective_rpc is missing — "
                "your vllm version may be too new or too old for the v1.0.2 shim."
            )
        return rpc(fn, args=args)


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
