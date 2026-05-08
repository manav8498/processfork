# processfork-criu

Linux-only CRIU adapter for ProcessFork. Promotes the world layer's
`procs` blob from `procs.unsupported.v1` (always-empty placeholder)
to `procs.criu.v1` — a real CRIU image bundle that captures live
process memory, file descriptors, and signal state, restorable on a
matching kernel.

## Status (v1.0.12)

- **Format ships, gating ships, the macOS skip path ships and is
  unit-tested.**
- **Live `criu dump` / `criu restore` validation requires Linux +
  the `criu` CLI binary + `CAP_SYS_ADMIN` (or root, or a
  pre-configured CRIU socket).** Not validated from the
  ProcessFork repo's CI host (macOS arm64). Operator must run the
  end-to-end check on a Linux box.
- This is the same shape as the Modal A10G lane for vLLM/SGLang:
  the code ships honest, the validation lane lives on a host the
  upstream CI doesn't have.

## Install

```bash
# Adapter package:
pip install processfork-criu

# CRIU binary (distro packages — pick one):
sudo apt install criu                  # Debian / Ubuntu
sudo dnf install criu                  # Fedora
sudo zypper install criu               # openSUSE
```

CRIU does not run on macOS, Windows, FreeBSD, or any non-Linux host.
`processfork_criu.is_available()` returns `False` everywhere except
Linux + `criu` on `$PATH`.

## Use

### As a library

```python
import processfork_criu as pfc

if not pfc.is_available():
    print(f"CRIU not available: {pfc.unavailable_reason()}")
    exit(0)

# Dump a running PID into a CRIU bundle (a tarball of CRIU images):
bundle: bytes = pfc.dump_pid(pid=12345, leave_running=True)

# ... later, on a matching kernel:
new_pid = pfc.restore_bundle(bundle, target_dir="/tmp/restore")
```

### Via `pf snapshot --criu-pid`

```bash
# Dump PID 12345 into the procs blob of a snapshot. World layer
# captures FS + env as usual; procs blob becomes a real
# procs.criu.v1 referencing the CRIU bundle digest.
pf snapshot --agent-id agent42 --fs-root /var/agent --criu-pid 12345

# Without --criu-pid the procs blob is procs.unsupported.v1 as
# before (unchanged behavior).
```

### What gets captured / what doesn't

CRIU captures:
- Process memory (anonymous mappings, file-backed mmap state).
- Open file descriptors + their seek positions, plus the
  underlying file paths so restore can reopen them.
- Signal handlers, signal masks, pending signals.
- TCP connections (with the kernel's `tcp-established` plugin —
  off by default; pass `tcp_established=True` to `dump_pid`).
- Process group / session IDs, terminal state.

CRIU does NOT capture:
- GPU device contexts (CUDA, ROCm) — bring those back via the
  vLLM/SGLang adapters that snapshot the engine state separately.
- Mounts / namespaces — handled if you also snapshot at the
  container level (LXC / runc / podman with their CRIU
  integration). Bare CRIU on a bare process leaves these alone.
- Anonymous shared memory between unrelated processes.

CRIU may refuse to dump:
- Processes with file descriptors open on `/dev/*` it doesn't
  recognize (custom devices).
- Processes in the middle of a system call that's not
  restartable (`futex` waits with weird flags, etc.).

In practice for AI agents: CRIU is excellent for snapshotting the
stateful parts of a Python agent process (heap, stdin/stdout pipes,
open log files, in-flight HTTP connections) so a restored worker
continues right where the original left off. It is the wrong tool
for snapshotting the engine's KV cache — that goes through the
vLLM/SGLang adapters' page-level capture instead, layered on top.

## What's tested where

- `tests/test_criu_adapter.py` runs everywhere:
  - `is_available()` returns False on macOS (and explains why).
  - `dump_pid()` / `restore_bundle()` raise `RuntimeError` with a
    clear message on non-Linux.
  - The bundle envelope format round-trips (header + body)
    independently of whether `criu` is installed.
- The end-to-end "spawn a Python process, dump it, kill it,
  restore it, continue" test runs only on Linux + `criu` CLI; it
  is gated by `pytest.importorskip` and the `is_available()`
  check, so the suite stays green on macOS CI.

## Honesty caveat

The maintainer of this repo runs CI on macOS arm64. The Linux +
CRIU end-to-end test path **has not been run in CI**; the unit
tests covering the format and gating have. If you deploy this in
production:

1. Run `pytest adapters/pf-criu/tests/ -v` on your Linux target
   first — `test_e2e_dump_restore_loops_back_a_value` is the
   one that exercises real `criu dump` + `criu restore`.
2. Pay attention to its skip/pass distinction: skipped =
   environment lacks `criu`, passed = real validation.

Same shape as the Modal vLLM lane: the code is committed; the
validation lives where the kernel lives.
