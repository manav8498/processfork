# SPDX-License-Identifier: MIT
"""Test suite for the processfork-criu adapter.

Three layers of tests:

1. **Always-on**: format round-trip, gating, error messages on non-
   Linux. These run on every CI host (macOS, Linux, even Windows
   if anyone wants to).

2. **Linux-only, no-CRIU-needed**: ``is_available()`` correctly
   reports `criu` not on $PATH when it isn't.

3. **Linux + criu binary**: end-to-end dump → kill → restore →
   continue. ``pytest.skip`` if criu isn't installed; otherwise
   exercise real ``criu dump`` and ``criu restore``.

The maintainer's CI is macOS arm64; only layer 1 runs there. Layer
3 needs an operator to run it on a Linux box. README has the
honesty caveat.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

import processfork_criu as pfc


# ---------------------------------------------------------------------------
# Layer 1 — always-on (format, gating, non-Linux error messages)
# ---------------------------------------------------------------------------

def test_module_version_matches_pyproject() -> None:
    """Adapter version must match the package version in
    pyproject.toml so wheels and runtime introspection agree."""
    assert pfc.__version__ == "1.0.13"


def test_procs_kind_constant_is_v1() -> None:
    """The on-disk procs blob marker is the contract with `pf
    verify`. Don't change without bumping the schema."""
    assert pfc.PROCS_KIND == "procs.criu.v1"
    assert pfc.SCHEMA_VERSION == 1


def test_bundle_serialization_round_trips() -> None:
    """The CRIU bundle envelope is header-line + raw tarball body.
    Byte-exact round-trip is the contract `pf snapshot --criu-pid`
    relies on for CAS deduplication."""
    bundle = pfc.CriuBundle(
        header={
            "kind": pfc.PROCS_KIND,
            "schema": 1,
            "pid": 12345,
            "leave_running": True,
            "tcp_established": False,
            "criu_version": "criu version 3.18",
            "kernel": "6.1.0-12-amd64",
            "machine": "x86_64",
            "captured_at": "2026-05-07T00:00:00+00:00",
        },
        tarball_bytes=b"\x1f\x8b\x08\x00fake-tarball-body",
    )
    serialized = bundle.serialize()

    # Header is line-1 JSON; body is everything after the first \n.
    nl = serialized.find(b"\n")
    assert nl > 0
    header = json.loads(serialized[:nl])
    assert header["kind"] == "procs.criu.v1"
    assert header["pid"] == 12345

    # Round-trip back through deserialize.
    again = pfc.CriuBundle.deserialize(serialized)
    assert again.header == bundle.header
    assert again.tarball_bytes == bundle.tarball_bytes


def test_deserialize_rejects_wrong_kind() -> None:
    """Defensive: don't accept blobs whose header says they're a
    different layer's format."""
    bad = (
        b'{"kind":"procs.unsupported.v1"}\n'
        + b"some bytes that look like a tarball"
    )
    with pytest.raises(ValueError, match="wrong kind"):
        pfc.CriuBundle.deserialize(bad)


def test_deserialize_rejects_missing_newline() -> None:
    bad = b'{"kind":"procs.criu.v1"} no newline here'
    with pytest.raises(ValueError, match="missing newline"):
        pfc.CriuBundle.deserialize(bad)


@pytest.mark.skipif(sys.platform.startswith("linux"), reason="non-Linux gate")
def test_is_available_false_on_non_linux() -> None:
    """The whole point of the gating: macOS / Windows / FreeBSD
    callers must get a clear False."""
    assert pfc.is_available() is False
    reason = pfc.unavailable_reason()
    assert reason is not None
    assert "Linux-only" in reason


@pytest.mark.skipif(sys.platform.startswith("linux"), reason="non-Linux gate")
def test_dump_pid_raises_on_non_linux() -> None:
    """Calling dump_pid on macOS must raise with a clear message,
    not silently produce a useless empty bundle."""
    with pytest.raises(RuntimeError, match="CRIU unavailable"):
        pfc.dump_pid(pid=1)


@pytest.mark.skipif(sys.platform.startswith("linux"), reason="non-Linux gate")
def test_restore_bundle_raises_on_non_linux() -> None:
    fake = pfc.CriuBundle(
        header={"kind": "procs.criu.v1", "schema": 1, "pid": 1},
        tarball_bytes=b"",
    ).serialize()
    with pytest.raises(RuntimeError, match="CRIU unavailable"):
        pfc.restore_bundle(fake)


# ---------------------------------------------------------------------------
# Layer 2 — Linux-only, no-criu-needed
# ---------------------------------------------------------------------------

@pytest.mark.skipif(not sys.platform.startswith("linux"), reason="Linux-only")
def test_is_available_reflects_criu_path_on_linux() -> None:
    """On Linux, is_available() == (criu binary on $PATH)."""
    import shutil
    has_criu = shutil.which("criu") is not None
    assert pfc.is_available() == has_criu
    if not has_criu:
        reason = pfc.unavailable_reason()
        assert reason is not None
        assert "criu" in reason.lower()


# ---------------------------------------------------------------------------
# Layer 3 — Linux + criu binary, end-to-end
# ---------------------------------------------------------------------------

@pytest.mark.skipif(
    not pfc.is_available(),
    reason=(
        "needs Linux + `criu` CLI on $PATH + CAP_SYS_ADMIN. "
        "Run on the operator's Linux target host."
    ),
)
def test_e2e_dump_restore_loops_back_a_value(tmp_path: Path) -> None:
    """Real CRIU dump+restore against a long-running Python process.

    The child process writes a sentinel value to a pipe, then loops
    forever. We dump it (--leave-running), kill the original PID,
    restore the bundle, and assert the restored process keeps
    writing — proving the whole capture survived the round-trip.

    NOTE: this test is the operator-runs-it validation. Skipped on
    macOS CI by ``pfc.is_available()``.
    """
    import os
    import signal
    import subprocess
    import time

    # Spawn a child that writes a heartbeat every 100 ms until it's
    # killed. The script itself is opaque to CRIU — we just need
    # *some* live process with a stable footprint.
    child_script = tmp_path / "heartbeat.py"
    pulse_log = tmp_path / "pulse.log"
    child_script.write_text(
        "import sys, time, os\n"
        f"with open({str(pulse_log)!r}, 'a') as f:\n"
        "    while True:\n"
        "        f.write(f'tick {os.getpid()} {time.time()}\\n')\n"
        "        f.flush()\n"
        "        time.sleep(0.1)\n"
    )
    proc = subprocess.Popen([sys.executable, str(child_script)])
    try:
        time.sleep(0.5)  # let it write a few heartbeats
        original_pid = proc.pid

        # Dump:
        bundle = pfc.dump_pid(pid=original_pid, leave_running=True)
        assert bundle.header["pid"] == original_pid
        assert bundle.header["kind"] == pfc.PROCS_KIND
        assert len(bundle.tarball_bytes) > 0

        # Kill the original; we want to prove restore brings it back.
        os.kill(original_pid, signal.SIGKILL)
        proc.wait(timeout=5)

        # Restore:
        time.sleep(0.2)
        new_pid = pfc.restore_bundle(bundle, target_dir=tmp_path / "restore")
        assert new_pid > 0
        assert new_pid != original_pid

        # Confirm the restored process keeps writing heartbeats.
        time.sleep(0.5)
        log_after = pulse_log.read_text()
        # Must contain at least one tick from new_pid.
        assert f"tick {new_pid}" in log_after, (
            f"no heartbeat from restored PID {new_pid} in:\n{log_after}"
        )

        # Cleanup.
        try:
            os.kill(new_pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
