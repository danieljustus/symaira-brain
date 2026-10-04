"""Owned file captures: CLI wait is bounded even if descendants keep stdout open."""
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

CLI_TIMEOUT = 15
CLEANUP_TIMEOUT = 2


def capture(binary, root, env, arguments, progress=None):
    case = progress.begin_cli(binary, arguments) if progress is not None else None
    # The durable owner is adjacent to the report, outside the product's private
    # filesystem observation. Without a journal, keep the same file-backed wait
    # in an independently owned temporary directory.
    if progress is not None:
        folder = progress.path.with_suffix(".cli") / str(case)
        folder.mkdir(parents=True, exist_ok=False)
        return _capture(binary, root, env, arguments, progress, case, folder)
    with tempfile.TemporaryDirectory(prefix="br-cli-") as temporary:
        return _capture(binary, root, env, arguments, None, None, Path(temporary))


def _capture(binary, root, env, arguments, progress, case, folder):
    paths = [folder / "stdout.bin", folder / "stderr.bin"]
    argv = [str(binary), *arguments]
    began = time.monotonic()
    child, failure, cleanup_error = None, None, None
    with paths[0].open("wb") as stdout, paths[1].open("wb") as stderr:
        try:
            if progress is not None:
                progress.event("cli.launch.begin", case=case, cwd=str(root),
                               stdout_file=str(paths[0]), stderr_file=str(paths[1]),
                               timeout_seconds=CLI_TIMEOUT)
            child = subprocess.Popen(argv, cwd=root, env=env, stdin=subprocess.DEVNULL,
                                     stdout=stdout, stderr=stderr)
            if progress is not None:
                progress.event("cli.spawned", case=case, pid=child.pid, cwd=str(root),
                               stdout_file=str(paths[0]), stderr_file=str(paths[1]),
                               timeout_seconds=CLI_TIMEOUT)
            child.wait(timeout=CLI_TIMEOUT)
        except BaseException as error:
            failure = error
        finally:
            if child is not None and child.poll() is None:
                try:
                    child.kill()
                    child.wait(timeout=CLEANUP_TIMEOUT)
                except (OSError, subprocess.TimeoutExpired) as error:
                    cleanup_error = repr(error)
    raw = [path.read_bytes() for path in paths]
    result = subprocess.CompletedProcess(argv, None if child is None else child.returncode, *raw)
    record = {"case": case, "pid": None if child is None else child.pid,
              "arguments_filesystem_base64": [base64.b64encode(os.fsencode(v)).decode() for v in argv],
              "cwd": str(root), "cwd_filesystem_base64": base64.b64encode(os.fsencode(root)).decode(),
              "stream_scope": "snapshot at owned CLI wait/cleanup; no descendant wire or exit claim",
              "begin_monotonic": began, "end_monotonic": time.monotonic(),
              "timeout_seconds": CLI_TIMEOUT, "timed_out": isinstance(failure, subprocess.TimeoutExpired),
              "exit": result.returncode, "failure": None if failure is None else repr(failure),
              "cleanup_error": cleanup_error,
              "streams": [{"path": str(path), "bytes": len(value), "sha256": hashlib.sha256(value).hexdigest(),
                           "base64": base64.b64encode(value).decode()} for path, value in zip(paths, raw)]}
    receipt = folder / "result.json"
    receipt.write_text(json.dumps(record, indent=2) + "\n")
    if progress is not None:
        progress.end_cli(case, result, pid=record["pid"], timed_out=record["timed_out"],
                         failure=record["failure"], cleanup_error=cleanup_error,
                         raw_receipt=str(receipt), raw_receipt_sha256=hashlib.sha256(receipt.read_bytes()).hexdigest())
    # A timed-out or failed child is evidence of failure, never an admitted CLI
    # outcome. Complete file-backed witnesses reach disk before this exception.
    if failure is not None:
        raise failure
    if cleanup_error is not None:
        raise AssertionError(f"CLI cleanup failed: {cleanup_error}")
    return result
