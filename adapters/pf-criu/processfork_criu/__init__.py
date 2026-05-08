# SPDX-License-Identifier: MIT
"""ProcessFork CRIU adapter — Linux in-flight subprocess capture.

Promotes the world layer's `procs` blob from `procs.unsupported.v1`
to `procs.criu.v1`. Honest about what we can validate from where:

- Format + gating + non-Linux skip path: unit-tested everywhere.
- `criu dump` / `criu restore` end-to-end: tested only on Linux +
  `criu` CLI installed (the e2e test pytest-skips otherwise).

See ``adapters/pf-criu/README.md`` for the full caveats.
"""
from __future__ import annotations

import io
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

__version__ = "1.0.14"

# v1.0.12 wire format. The on-disk procs blob looks like:
#
#   {"kind":"procs.criu.v1","schema":1, ...header fields...}\n
#   <raw tarball bytes of the CRIU images directory>
#
# `pf snapshot --criu-pid` builds this header + body and writes it
# as a single CAS blob. `pf checkout` is read-only at the procs
# layer (you don't restore live processes from `pf checkout`); the
# operator opts in to restore via this adapter's ``restore_bundle``.
PROCS_KIND = "procs.criu.v1"
SCHEMA_VERSION = 1


@dataclass(frozen=True)
class CriuBundle:
    """In-memory CRIU bundle: a header dict + raw tarball body.

    Attributes:
        header: JSON-serializable metadata describing the dump
            (CRIU version, kernel, capture timestamp, dumped PID,
            kept-running flag, original argv if available).
        tarball_bytes: A tarball of the CRIU images directory
            produced by `criu dump --images-dir`. CRIU writes a
            handful of `.img` files + a `pages-*.img`; we tar them
            so the whole thing is one CAS blob.
    """

    header: dict[str, Any]
    tarball_bytes: bytes

    def serialize(self) -> bytes:
        """Header line + body, exactly what gets stored as the
        ``procs.criu.v1`` CAS blob."""
        return (
            json.dumps(self.header, separators=(",", ":")).encode("utf-8")
            + b"\n"
            + self.tarball_bytes
        )

    @classmethod
    def deserialize(cls, data: bytes) -> "CriuBundle":
        nl = data.find(b"\n")
        if nl < 0:
            raise ValueError("CRIU bundle missing newline-delimited header")
        header = json.loads(data[:nl].decode("utf-8"))
        if header.get("kind") != PROCS_KIND:
            raise ValueError(
                f"CRIU bundle wrong kind: {header.get('kind')!r} != {PROCS_KIND!r}"
            )
        return cls(header=header, tarball_bytes=data[nl + 1 :])


# ---------------------------------------------------------------------------
# Availability gating
# ---------------------------------------------------------------------------

def is_available() -> bool:
    """Return True iff ``dump_pid`` / ``restore_bundle`` will work
    on this host. False on macOS, Windows, FreeBSD; False on Linux
    if `criu` isn't on $PATH."""
    if not _is_linux():
        return False
    return _criu_path() is not None


def unavailable_reason() -> str | None:
    """Human-readable string explaining *why* CRIU isn't available
    here, or None if it is."""
    if not _is_linux():
        return f"CRIU is Linux-only; this host runs {platform.system()!r}"
    if _criu_path() is None:
        return (
            "CRIU CLI not found on $PATH. Install with "
            "`apt install criu` (Debian/Ubuntu), `dnf install criu` "
            "(Fedora), or `zypper install criu` (openSUSE)."
        )
    return None


def _is_linux() -> bool:
    return sys.platform.startswith("linux")


def _criu_path() -> str | None:
    return shutil.which("criu")


def _criu_version() -> str | None:
    """Run `criu --version` and return the first line. None if criu
    isn't on $PATH or the call fails."""
    path = _criu_path()
    if path is None:
        return None
    try:
        out = subprocess.run(
            [path, "--version"], capture_output=True, text=True, timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return (out.stdout or out.stderr).splitlines()[0].strip() if (out.stdout or out.stderr) else None


# ---------------------------------------------------------------------------
# Dump / Restore
# ---------------------------------------------------------------------------

def dump_pid(
    pid: int,
    *,
    leave_running: bool = True,
    tcp_established: bool = False,
    extra_args: list[str] | None = None,
) -> CriuBundle:
    """Snapshot the live process at ``pid`` via `criu dump`.

    Args:
        pid: Target process ID. The CRIU CLI must have permission
            to ptrace it — typically same UID, or
            ``CAP_SYS_PTRACE``.
        leave_running: If True (default), pass ``--leave-running``
            so the original process keeps running after the dump.
            If False, ``criu dump`` SIGKILLs the target after
            writing the images (the standard CRIU behavior; useful
            for failover snapshots where you want to retire the
            old PID).
        tcp_established: If True, pass ``--tcp-established`` so
            CRIU dumps active TCP sockets. Off by default because
            it requires the kernel `repair_mode` capability; many
            cloud kernels disallow it.
        extra_args: Additional flags forwarded verbatim to
            ``criu dump``. Use with care — wrong combos can leave
            the target half-frozen.

    Returns:
        ``CriuBundle`` whose ``serialize()`` is what ``pf snapshot
        --criu-pid`` writes into the procs blob.

    Raises:
        RuntimeError: On non-Linux, missing `criu`, dump failure,
            or bad PID.
    """
    if reason := unavailable_reason():
        raise RuntimeError(f"CRIU unavailable: {reason}")
    if pid <= 0:
        raise RuntimeError(f"bad PID: {pid}")

    with tempfile.TemporaryDirectory(prefix="pf-criu-dump-") as workdir:
        images_dir = Path(workdir) / "images"
        images_dir.mkdir()

        argv = [
            _criu_path(),
            "dump",
            "--tree", str(pid),
            "--images-dir", str(images_dir),
            "--shell-job",   # accept inherited tty fds (most agents have these)
        ]
        if leave_running:
            argv.append("--leave-running")
        if tcp_established:
            argv.append("--tcp-established")
        if extra_args:
            argv.extend(extra_args)

        proc = subprocess.run(argv, capture_output=True, text=True, check=False)
        if proc.returncode != 0:
            raise RuntimeError(
                f"criu dump failed (exit {proc.returncode}):\n"
                f"  stdout: {proc.stdout.strip()}\n"
                f"  stderr: {proc.stderr.strip()}"
            )

        tar_buf = io.BytesIO()
        with tarfile.open(fileobj=tar_buf, mode="w") as tar:
            tar.add(images_dir, arcname="images", recursive=True)
        tarball = tar_buf.getvalue()

    header = {
        "kind": PROCS_KIND,
        "schema": SCHEMA_VERSION,
        "pid": int(pid),
        "leave_running": bool(leave_running),
        "tcp_established": bool(tcp_established),
        "criu_version": _criu_version() or "",
        "kernel": platform.release(),
        "machine": platform.machine(),
        "captured_at": _utc_now_isoformat(),
    }
    return CriuBundle(header=header, tarball_bytes=tarball)


def restore_bundle(
    bundle: CriuBundle | bytes,
    *,
    target_dir: str | Path | None = None,
    extra_args: list[str] | None = None,
) -> int:
    """Restore a previously-dumped process via `criu restore`.

    Args:
        bundle: A ``CriuBundle`` (in-memory) or raw bytes (the
            on-disk ``procs.criu.v1`` blob).
        target_dir: Where to extract the CRIU images before
            restoring. If None, a tempdir is used and CRIU's
            standard behavior applies (target_dir cleanup is up
            to the caller; we do not delete it because the
            restored process keeps file descriptors open into it).
        extra_args: Additional flags forwarded to ``criu restore``.

    Returns:
        PID of the restored process, as reported by `criu restore
        --pidfile`.

    Raises:
        RuntimeError: On non-Linux, missing criu, or restore failure.
    """
    if reason := unavailable_reason():
        raise RuntimeError(f"CRIU unavailable: {reason}")

    if isinstance(bundle, (bytes, bytearray)):
        bundle = CriuBundle.deserialize(bytes(bundle))

    if target_dir is None:
        target_dir = Path(tempfile.mkdtemp(prefix="pf-criu-restore-"))
    else:
        target_dir = Path(target_dir)
        target_dir.mkdir(parents=True, exist_ok=True)

    images_dir = target_dir / "images"
    with tarfile.open(fileobj=io.BytesIO(bundle.tarball_bytes), mode="r") as tar:
        # Strip the leading `images/` arcname to land directly in
        # `target_dir/images`. tarfile.extractall doesn't support
        # arcname rewriting natively; do it manually.
        for member in tar.getmembers():
            if not member.name.startswith("images"):
                continue
            relative = member.name.removeprefix("images").lstrip("/")
            dest = images_dir / relative if relative else images_dir
            if member.isdir():
                dest.mkdir(parents=True, exist_ok=True)
                continue
            dest.parent.mkdir(parents=True, exist_ok=True)
            f = tar.extractfile(member)
            if f is None:
                continue
            with open(dest, "wb") as out:
                shutil.copyfileobj(f, out)

    pidfile = target_dir / "restored.pid"
    argv = [
        _criu_path(),
        "restore",
        "--images-dir", str(images_dir),
        "--shell-job",
        "--pidfile", str(pidfile),
        "--restore-detached",
    ]
    if bundle.header.get("tcp_established"):
        argv.append("--tcp-established")
    if extra_args:
        argv.extend(extra_args)

    proc = subprocess.run(argv, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(
            f"criu restore failed (exit {proc.returncode}):\n"
            f"  stdout: {proc.stdout.strip()}\n"
            f"  stderr: {proc.stderr.strip()}"
        )

    try:
        return int(pidfile.read_text().strip())
    except (OSError, ValueError) as e:
        raise RuntimeError(f"criu restore did not write {pidfile}: {e}") from e


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _utc_now_isoformat() -> str:
    """RFC-3339 UTC timestamp without fractional seconds. Module-
    private so the test suite can monkeypatch it for determinism."""
    import datetime as _dt
    return _dt.datetime.now(tz=_dt.timezone.utc).replace(microsecond=0).isoformat()


__all__ = [
    "CriuBundle",
    "PROCS_KIND",
    "SCHEMA_VERSION",
    "dump_pid",
    "is_available",
    "restore_bundle",
    "unavailable_reason",
    "__version__",
]
