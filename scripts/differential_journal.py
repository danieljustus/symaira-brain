"""Opt-in durable child diagnostics; never participates in parity projections."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


class Journal:
    def __init__(self) -> None:
        value = os.environ.get("SYMBRAIN_DIFFERENTIAL_JOURNAL")
        self.root = Path(value).resolve() if value else None
        if self.root:
            # Refuse stale output roots rather than overwrite an earlier failed attempt.
            self.root.mkdir(parents=True, exist_ok=False)
        self.sequence = 0

    def event(self, phase: str, **fields) -> None:
        if self.root is None:
            return
        row = {"sequence": self.sequence, "phase": phase,
               "unix_ns": time.time_ns(), "monotonic_ns": time.monotonic_ns(), **fields}
        self.sequence += 1
        with (self.root / "phases.jsonl").open("a", encoding="utf-8") as stream:
            stream.write(json.dumps(row, ensure_ascii=True) + "\n")
            stream.flush()
            os.fsync(stream.fileno())

    def bind(self, runtime: str, binary: Path) -> None:
        if self.root is not None:
            self.event("binary", runtime=runtime, path=str(binary),
                       sha256=hashlib.sha256(binary.read_bytes()).hexdigest())

    def streams(self, case: str, runtime: str, stdout: bytes, stderr: bytes) -> dict:
        paths = {}
        for name, data in (("stdout", stdout), ("stderr", stderr)):
            path = self.root / f"{case}-{runtime}.{name}"
            with path.open("wb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            paths[name] = {"path": str(path), "bytes": len(data),
                           "sha256": hashlib.sha256(data).hexdigest()}
        return paths

    def run(self, case: str, runtime: str, execute, binary: Path, argv, env, stdin, pty):
        if self.root is None:
            return execute(binary, argv, env, stdin, pty)
        self.event("child_begin", case=case, runtime=runtime, binary=str(binary),
                   argv=[{"hex": arg.hex()} if isinstance(arg, bytes) else arg for arg in argv],
                   cwd=env.get("PROJECT"), timeout_seconds=10, pty=pty)
        started = time.monotonic_ns()
        try:
            result = execute(binary, argv, env, stdin, pty)
        except subprocess.TimeoutExpired as error:
            self.event("child_timeout", case=case, runtime=runtime,
                       elapsed_ns=time.monotonic_ns()-started, timeout_seconds=error.timeout,
                       **self.streams(case, runtime, error.stdout or b"", error.stderr or b""))
            raise
        except BaseException as error:
            self.event("child_exception", case=case, runtime=runtime,
                       elapsed_ns=time.monotonic_ns()-started, error=repr(error))
            raise
        self.event("child_end", case=case, runtime=runtime, returncode=result.returncode,
                   elapsed_ns=time.monotonic_ns()-started,
                   **self.streams(case, runtime, result.stdout, result.stderr))
        return result

    def fixture_state(self, case: str, runtime: str, fixture: Path) -> None:
        if self.root is None:
            return
        assert not self.root.is_relative_to(fixture.parent), "journal must be outside disposable fixture"
        # Metadata only: avoid reading database or configuration while a descendant may own it.
        files = []
        for path in sorted(fixture.rglob("*")):
            try:
                stat = path.lstat()
                files.append({"path": str(path.relative_to(fixture)), "size": stat.st_size,
                              "mtime_ns": stat.st_mtime_ns, "mode": stat.st_mode})
            except OSError as error:
                files.append({"path": str(path.relative_to(fixture)), "error": repr(error)})
        self.event("fixture_state", case=case, runtime=runtime, files=files)
