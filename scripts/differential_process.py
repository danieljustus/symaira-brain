"""Deadline-bound child execution for the black-box CLI differential (#946).

Every case child, PTY or pipe, runs in its own process group. The deadline covers
the whole run (reads included), and the group, not only the direct child, is
SIGKILLed when the deadline passes or when the leader exits and leaves
descendants behind. A hang is reported as `subprocess.TimeoutExpired` so the
harness keeps it as a failure instead of letting a stray grandchild outlive its
case and disturb later ones.
"""
from __future__ import annotations

import os
import select
import signal
import subprocess
import time
from pathlib import Path

RUN_TIMEOUT_SECONDS = 10.0
_POLL_SECONDS = 0.05

if os.name == "posix":
    import pty


def kill_group(process: subprocess.Popen) -> None:
    """SIGKILL the child's whole process group; a group that is already gone is fine.

    `process_group=0` makes the child its own group leader, so its pid is the
    group id. The group id cannot be recycled while any member is still alive,
    and an empty group answers ESRCH.
    """
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass


def _command(binary: Path, argv: tuple) -> list:
    return [str(binary), *argv]


def run_child(
    binary: Path,
    argv: tuple,
    env: dict[str, str],
    stdin: bytes | None = None,
    use_pty: bool = False,
    timeout: float = RUN_TIMEOUT_SECONDS,
) -> subprocess.CompletedProcess[bytes]:
    if use_pty:
        return _run_pty(binary, argv, env, timeout)
    return _run_pipes(binary, argv, env, stdin, timeout)


def _run_pipes(binary, argv, env, stdin, timeout) -> subprocess.CompletedProcess[bytes]:
    command = _command(binary, argv)
    process = subprocess.Popen(
        command, env=env, cwd=env.get("PROJECT"), process_group=0,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    pending: bytes | None = stdin if stdin is not None else b""
    deadline = time.monotonic() + timeout
    swept = False
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(command, timeout)
            try:
                stdout, stderr = process.communicate(pending, timeout=min(remaining, _POLL_SECONDS))
                break
            except subprocess.TimeoutExpired:
                pending = None  # the input was handed over on the first call
                if process.poll() is not None and not swept:
                    kill_group(process)  # leader is gone; a descendant may still hold the pipes
                    swept = True
    except subprocess.TimeoutExpired:
        kill_group(process)
        stdout, stderr = process.communicate()  # group is dead, so the pipes reach EOF
        raise subprocess.TimeoutExpired(command, timeout, output=stdout, stderr=stderr) from None
    except BaseException:
        kill_group(process)
        process.communicate()
        raise
    kill_group(process)  # descendants that outlived the leader must not leak into later cases
    return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)


def _run_pty(binary, argv, env, timeout) -> subprocess.CompletedProcess[bytes]:
    command = _command(binary, argv)
    master, slave = pty.openpty()
    try:
        process = subprocess.Popen(
            command, env=env, cwd=env.get("PROJECT"), process_group=0,
            stdin=slave, stdout=slave, stderr=slave,
        )
    finally:
        os.close(slave)
    chunks: list[bytes] = []
    deadline = time.monotonic() + timeout
    swept = False
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                kill_group(process)
                process.wait()
                raise subprocess.TimeoutExpired(command, timeout, output=b"".join(chunks))
            if not select.select([master], [], [], min(remaining, _POLL_SECONDS))[0]:
                if process.poll() is not None and not swept:
                    kill_group(process)  # leader is gone; a descendant may still hold the terminal
                    swept = True
                continue
            try:
                chunk = os.read(master, 4096)
            except OSError:  # EIO: every slave end is closed
                break
            if not chunk:
                break
            chunks.append(chunk)
        returncode = process.wait(timeout=max(deadline - time.monotonic(), _POLL_SECONDS))
        kill_group(process)
    except subprocess.TimeoutExpired:
        kill_group(process)
        process.wait()
        raise
    except BaseException:
        kill_group(process)
        process.wait()
        raise
    finally:
        os.close(master)
    return subprocess.CompletedProcess(command, returncode, b"".join(chunks), b"")
