#!/usr/bin/env python3
"""Byte-compare selected Go and Rust Browse CLI contracts in isolation."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import threading
from pathlib import Path
from typing import Any


MAX_CAPTURE_BYTES = 1 << 20
PREVIEW_BYTES = 256
SESSION = "default"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run_process(binary: Path, argv: list[str], env: dict[str, str], stdin: bytes = b"") -> dict[str, Any]:
    try:
        result = subprocess.run(
            [str(binary), *argv], input=stdin, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, env=env, timeout=20, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"error": str(error), "returncode": None, "stdout": b"", "stderr": b""}
    if len(result.stdout) > MAX_CAPTURE_BYTES or len(result.stderr) > MAX_CAPTURE_BYTES:
        return {"error": "CLI output exceeded the 1 MiB harness bound", "returncode": result.returncode,
                "stdout": result.stdout[:MAX_CAPTURE_BYTES], "stderr": result.stderr[:MAX_CAPTURE_BYTES]}
    return {"returncode": result.returncode, "stdout": result.stdout, "stderr": result.stderr}


def output_record(result: dict[str, Any]) -> dict[str, Any]:
    record: dict[str, Any] = {"returncode": result["returncode"]}
    if result.get("error"):
        record["error"] = result["error"]
    for field in ("stdout", "stderr"):
        value = result[field]
        record[field] = {"bytes": len(value), "sha256": sha256(value),
                         "preview": value[:PREVIEW_BYTES].decode("utf-8", "backslashreplace")}
    return record


def command_names(help_bytes: bytes, *, root: bool) -> list[str]:
    text = help_bytes.decode("utf-8", "replace")
    names: list[str] = []
    in_commands = root
    for line in text.splitlines():
        if line.strip() == "Available Commands:":
            in_commands = True
            continue
        if not root and in_commands and line and not line.startswith(" "):
            break
        if in_commands:
            match = re.match(r"^\s{2}([A-Za-z0-9][A-Za-z0-9_-]*)\s{2,}\S", line)
            if match:
                names.append(match.group(1))
        if line.strip() in {"Flags:", "Global Flags:", "Use \"symbrowse [command] --help\" for more information about a command."}:
            if root:
                in_commands = False
            elif line.strip() == "Flags:":
                break
    return names


def help_tree(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    pending: list[tuple[str, ...]] = [()]
    visited: set[tuple[str, ...]] = set()
    comparisons = []
    while pending:
        path = pending.pop(0)
        if path in visited:
            continue
        visited.add(path)
        args = [*path, "--help"]
        go_result = run_process(go, args, env)
        rust_result = run_process(rust, args, env)
        matched = all(go_result.get(k) == rust_result.get(k) for k in ("returncode", "stdout", "stderr"))
        comparisons.append({"case": "CLI-001", "argv": args, "matched": matched,
                            "go": output_record(go_result), "rust": output_record(rust_result)})
        if go_result["returncode"] == 0:
            children = command_names(go_result["stdout"], root=not path)
            pending.extend((*path, name) for name in children if name not in {"help"})
    return comparisons


class UnixDaemonStub:
    def __init__(self, path: Path, *, status_probe: bool):
        self.path = path
        self.frame: dict[str, Any] | None = None
        self.error: str | None = None
        self.status_probe = status_probe
        self.thread: threading.Thread | None = None
        self.listener: socket.socket | None = None

    def __enter__(self) -> "UnixDaemonStub":
        if os.name != "posix":
            raise RuntimeError("the local CLI daemon stub currently requires Unix sockets")
        self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(str(self.path))
        self.listener.listen(1)
        self.listener.settimeout(5)
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()
        return self

    def _serve(self) -> None:
        assert self.listener is not None
        try:
            request_count = 2 if self.status_probe else 1
            for request_index in range(request_count):
                connection, _ = self.listener.accept()
                with connection:
                    connection.settimeout(5)
                    raw = bytearray()
                    while b"\n" not in raw:
                        chunk = connection.recv(65536)
                        if not chunk:
                            break
                        raw.extend(chunk)
                        if len(raw) > MAX_CAPTURE_BYTES:
                            raise RuntimeError("daemon request exceeded the 1 MiB bound")
                    frame = json.loads(bytes(raw).split(b"\n", 1)[0])
                    if request_index == 0:
                        self.frame = frame
                    if frame.get("cmd") == "daemon.status":
                        data = {"session": frame.get("session", "default")}
                    else:
                        args = frame.get("args") or {}
                        data = {"url": args.get("url", ""), "value": 2,
                                "expression": args.get("expression", "")}
                    response = {"success": True, "data": data, "warnings": []}
                    connection.sendall(json.dumps(response, separators=(",", ":")).encode() + b"\n")
        except Exception as error:  # retained in the bounded report
            self.error = str(error)

    def __exit__(self, *_: object) -> None:
        if self.thread is not None:
            self.thread.join(timeout=16)
        if self.listener is not None:
            self.listener.close()
        try:
            self.path.unlink()
        except FileNotFoundError:
            pass


def make_env(root: Path) -> dict[str, str]:
    home = root / "home"
    runtime = root / "runtime"
    config = root / "config"
    state = root / "state"
    cache = root / "cache"
    temp = root / "tmp"
    for path in (home, runtime, config, state, cache, temp):
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8", "TZ": "UTC", "HOME": str(home),
        "USERPROFILE": str(home), "XDG_RUNTIME_DIR": str(runtime),
        "XDG_CONFIG_HOME": str(config), "XDG_CACHE_HOME": str(cache),
        "XDG_STATE_HOME": str(state), "TMPDIR": str(temp),
        "SYMBROWSE_CONFIG_DIR": str(config / "symbrowse"),
        "SYMBROWSE_STATE_DIR": str(state / "symbrowse"),
        "SYMBROWSE_CACHE_DIR": str(cache / "symbrowse"),
        "SYMBROWSE_USER_DATA_DIR": str(root / "user-data"),
        "SYMBROWSE_NO_AUTOSTART": "1",
    }
    (root / "user-data").mkdir(mode=0o700, exist_ok=True)
    return env


def socket_path(env: dict[str, str]) -> Path:
    if sys.platform == "darwin":
        return Path(env["HOME"]) / "Library/Caches/symbrowse/run" / f"{SESSION}.sock"
    return Path(env["XDG_RUNTIME_DIR"]) / "symbrowse" / f"{SESSION}.sock"


def compare_processes(go: Path, rust: Path, argv: list[str], stdin: bytes,
                      env: dict[str, str], *, stub: bool) -> dict[str, Any]:
    go_frame = rust_frame = None
    if stub:
        if os.name != "posix":
            return {"case": "unsupported", "argv": argv,
                    "reason": "Windows named-pipe daemon stub is not implemented"}
        go_path = socket_path(env)
        with UnixDaemonStub(go_path, status_probe=False) as go_stub:
            go_result = run_process(go, argv, env, stdin)
        go_frame = go_stub.frame
        go_error = go_stub.error
        rust_path = socket_path(env)
        with UnixDaemonStub(rust_path, status_probe=True) as rust_stub:
            rust_result = run_process(rust, argv, env, stdin)
        rust_frame = rust_stub.frame
        rust_error = rust_stub.error
    else:
        go_result = run_process(go, argv, env, stdin)
        rust_result = run_process(rust, argv, env, stdin)
        go_error = rust_error = None
    matched = all(go_result.get(k) == rust_result.get(k) for k in ("returncode", "stdout", "stderr"))
    row = {"argv": argv, "matched": matched, "go": output_record(go_result),
           "rust": output_record(rust_result)}
    if stub:
        def payload(frame: dict[str, Any] | None) -> dict[str, Any] | None:
            if frame is None:
                return None
            return {key: frame.get(key) for key in ("cmd", "session", "args")}

        row.update({"daemon_frames_match": go_frame == rust_frame,
                    "go_frame": go_frame, "rust_frame": rust_frame,
                    "go_stub_error": go_error, "rust_stub_error": rust_error})
        row["daemon_payloads_match"] = payload(go_frame) == payload(rust_frame)
        row["matched"] = bool(row["matched"] and row["daemon_payloads_match"] and not go_error and not rust_error)
    return row


def run_fixed_cases(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    cases = [
        ("CLI-002", ["goto", "https://fixture.invalid", "--json"], b"", True),
        ("CLI-002", ["open", "https://fixture.invalid", "--json"], b"", True),
        ("CLI-003", ["state", "save"], b"", False),
        ("CLI-003", ["state", "save", "one", "extra"], b"", False),
        ("CLI-003", ["version", "extra"], b"", False),
        ("CLI-003", ["version", "--", "--json"], b"", False),
        ("CLI-004", ["--json", "config", "show"], b"", False),
        ("CLI-004", ["config", "--json", "show"], b"", False),
        ("CLI-004", ["config", "show", "--json"], b"", False),
        ("CLI-004", ["config", "show", "--output", "json"], b"", False),
        ("CLI-004", ["config", "--output=json", "show", "--json"], b"", False),
        ("CLI-004", ["config", "show", "--output", "yaml", "--output=text"], b"", False),
        ("CLI-004", ["config", "show", "--", "--json"], b"", False),
        ("CLI-005", ["eval", "1+1", "--json"], b"", True),
        ("CLI-005", ["eval", "1+1"], b"", True),
        ("CLI-005", ["eval", "ignored", "--stdin", "--json"], b"document.title", True),
        ("CLI-005", ["eval", "ZG9jdW1lbnQudGl0bGU=", "--base64", "--json"], b"", True),
        ("CLI-005", ["eval", "--stdin", "--base64", "--json"], b"MSsy", True),
        ("CLI-005", ["eval", "1+1", "extra", "--json"], b"", True),
        ("CLI-005", ["eval", "--json"], b"", False),
        ("CLI-005", ["eval", "!!!", "--base64", "--json"], b"", False),
    ]
    comparisons = []
    for contract, argv, stdin, stub in cases:
        row = compare_processes(go, rust, argv, stdin, env, stub=stub)
        row["case"] = contract
        comparisons.append(row)
    return comparisons


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", required=True, type=Path, help="built Go symbrowse executable")
    parser.add_argument("--rust", required=True, type=Path, help="built Rust symbrowse executable")
    parser.add_argument("--report", required=True, type=Path, help="bounded JSON comparison report")
    parser.add_argument("--temp-root", type=Path, help="external NVMe temp directory on local macOS")
    parser.add_argument("--skip-help-tree", action="store_true", help="skip exhaustive CLI-001 help traversal")
    args = parser.parse_args()
    for label, binary in (("Go", args.go), ("Rust", args.rust)):
        if binary.is_symlink() or not binary.is_file():
            parser.error(f"{label} binary must be an existing regular file: {binary}")
    root = args.temp_root or (Path(os.environ["SYMAIRA_EXTERNAL_RUNTIME_ROOT"])
                              if sys.platform == "darwin" and os.environ.get("SYMAIRA_EXTERNAL_RUNTIME_ROOT")
                              else Path(tempfile.gettempdir()))
    if sys.platform == "darwin" and not os.environ.get("CI"):
        nvme = Path("/Volumes/1TB_NVMe_SN850X").resolve(strict=True)
        if not root.resolve(strict=True).is_relative_to(nvme):
            parser.error(f"local macOS --temp-root must be on external NVMe: {nvme}")
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    common = root / f"cli-base-{os.getpid()}"
    common.mkdir(mode=0o700)
    env = make_env(common)
    rows = [] if args.skip_help_tree else help_tree(args.go.resolve(), args.rust.resolve(), env)
    rows.extend(run_fixed_cases(args.go.resolve(), args.rust.resolve(), env))
    report = {
        "schema_version": 1,
        "source_head": subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip(),
        "go_binary_sha256": sha256(args.go.read_bytes()),
        "rust_binary_sha256": sha256(args.rust.read_bytes()),
        "platform": sys.platform,
        "isolated_env_root": str(common),
        "rows": rows,
        "summary": {"cases": len(rows), "matched": sum(row.get("matched") is True for row in rows),
                    "mismatched": sum(row.get("matched") is False for row in rows),
                    "unsupported": sum(row.get("case") == "unsupported" for row in rows)},
        "parity_established": False,
    }
    args.report.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report["summary"], sort_keys=True))
    print(f"report: {args.report}")
    return 0 if report["summary"]["mismatched"] == 0 and report["summary"]["unsupported"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
