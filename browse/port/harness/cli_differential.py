#!/usr/bin/env python3
"""Byte-compare selected Go and Rust Browse CLI contracts in isolation."""
from __future__ import annotations

import argparse
import base64
import ctypes
import hashlib
import json
import os
import re
import selectors
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any
from ctypes import wintypes


MAX_CAPTURE_BYTES = 1 << 20
PREVIEW_BYTES = 256
SESSION = "default"
TRACE_INTERACTIONS = {
    "click", "dblclick", "hover", "focus", "check", "uncheck", "scrollintoview",
    "scroll", "fill", "type", "select", "press",
}


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


def completion_oracle_cases(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    """Compare complete native Go Cobra scripts with Rust output on each runner."""
    rows = []
    for shell in ("bash", "zsh", "fish", "powershell"):
        argv = ["completion", shell]
        go_result = run_process(go, argv, env)
        rust_result = run_process(rust, argv, env)
        stdout = go_result["stdout"]
        rust_stdout = rust_result["stdout"]
        rows.append({
            "case": "CLI-completion-oracle",
            "shell": shell,
            "argv": argv,
            "matched": (go_result.get("returncode") == rust_result.get("returncode") == 0
                        and not go_result.get("error") and not rust_result.get("error")
                        and stdout == rust_stdout and not go_result["stderr"] and not rust_result["stderr"]),
            "criterion": "exact script bytes match the native Go Cobra oracle",
            "returncode": go_result.get("returncode"),
            "rust_returncode": rust_result.get("returncode"),
            "error": go_result.get("error"),
            "rust_error": rust_result.get("error"),
            "stdout_bytes": len(stdout),
            "stdout_sha256": sha256(stdout),
            "stdout_base64": base64.b64encode(stdout).decode("ascii"),
            "rust_stdout_bytes": len(rust_stdout),
            "rust_stdout_sha256": sha256(rust_stdout),
            "rust_stdout_base64": base64.b64encode(rust_stdout).decode("ascii"),
            "stderr": output_record(go_result)["stderr"],
            "rust_stderr": output_record(rust_result)["stderr"],
        })
    return rows


def completion_candidate_cases(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    """Exercise implemented Rust command and flag completion via Cobra's hidden protocol."""
    cases = [
        ("cookies-child-commands", ["__complete", "cookies", ""]),
        ("cookies-list-flags", ["__complete", "cookies", "list", "--"]),
    ]
    rows = []
    for name, argv in cases:
        go_result = run_process(go, argv, env)
        rust_result = run_process(rust, argv, env)
        rows.append({
            "case": "CLI-completion-candidates",
            "name": name,
            "argv": argv,
            "matched": (go_result.get("returncode") == rust_result.get("returncode") == 0
                        and not go_result.get("error") and not rust_result.get("error")
                        and go_result["stdout"] == rust_result["stdout"]),
            "criterion": "implemented subtree candidate bytes match Go __complete output",
            "go": output_record(go_result),
            "go_stdout_base64": base64.b64encode(go_result["stdout"]).decode("ascii"),
            "rust": output_record(rust_result),
            "rust_stdout_base64": base64.b64encode(rust_result["stdout"]).decode("ascii"),
        })
    return rows


def json_payload(result: dict[str, Any]) -> tuple[Any, str | None]:
    try:
        return json.loads(result["stdout"]), None
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        return None, str(error)


def trace_replay_result(steps: list[dict[str, Any]]) -> dict[str, Any]:
    outcomes: list[dict[str, Any]] = []
    matched = failed = deviated = 0
    for index, step in enumerate(steps):
        command = step.get("command", "")
        outcome: dict[str, Any] = {"index": index, "command": command, "matched": False}
        if command in ("open", "goto"):
            expected = step.get("expected_url", "")
            actual = step.get("url", "")
            if actual.endswith("/failed"):
                outcome["error"] = "fixture navigation failed"
            else:
                outcome.update({"expected_url": expected, "actual_url": actual,
                                "matched": normalize_trace_fixture_url(actual)
                                == normalize_trace_fixture_url(expected)})
        elif command in TRACE_INTERACTIONS:
            outcome["matched"] = True
        elif command == "auth.login":
            outcome["error"] = "credential step requires symvault re-resolution; replay it with auth login"
        else:
            outcome["error"] = f'step command "{command}" is not replayable'
        if outcome.get("error"):
            failed += 1
        elif outcome["matched"]:
            matched += 1
        else:
            deviated += 1
        outcomes.append(outcome)
    return {"total": len(steps), "matched": matched, "deviated": deviated,
            "failed": failed, "outcomes": outcomes}


def normalize_trace_fixture_url(value: str) -> str:
    value = value.split("#", 1)[0]
    return value[:-1] if value.endswith("/") else value


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
    for path in [("help",), ("help", "scroll")]:
        args = list(path)
        go_result = run_process(go, args, env)
        rust_result = run_process(rust, args, env)
        matched = all(go_result.get(k) == rust_result.get(k) for k in ("returncode", "stdout", "stderr"))
        comparisons.append({"case": "CLI-001-help-alias", "argv": args, "matched": matched,
                            "go": output_record(go_result), "rust": output_record(rust_result)})
    return comparisons


def implemented_help(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    expected = {
        "a11y", "back", "batch", "cache", "check", "click", "config", "console", "daemon", "doctor", "downloads", "errors", "dblclick", "dialog", "eval", "fetch", "fill", "find",
        "flow", "focus", "forward", "frame", "get", "goto", "help", "hover", "is", "journal", "mcp", "open", "policy", "press", "profiles",
        "read", "reload", "screenshot", "scroll", "scrollintoview", "select", "session", "set", "snapshot", "state", "storage", "cookies", "tab", "tools", "trace", "diff", "network", "type", "uncheck", "upload", "version", "wait", "watch", "workflow",
    }
    go_root = run_process(go, ["--help"], env)
    rust_root = run_process(rust, ["--help"], env)
    go_commands = set(command_names(go_root["stdout"], root=True))
    rust_help = rust_root["stdout"].decode("utf-8", "replace")
    match = go_root["returncode"] == 0 and rust_root["returncode"] == 0
    advertised: set[str] = set()
    advertised = set(command_names(rust_root["stdout"], root=True))
    # scrollintoview is a runnable Go command with exact help, but Cobra omits
    # it from the grouped root listing in the current Go binary.
    go_visible = advertised - {"fetch", "tools", "workflow", "scrollintoview"}
    match = match and advertised == expected and go_visible <= go_commands
    rows = [{"case": "CLI-001-supported", "argv": ["--help"], "matched": match,
             "criterion": "Rust root help lists its real command/help routes; shared advertised commands exist in Go",
             "go_commands": sorted(go_commands), "rust_commands": sorted(advertised),
             "full_go_root_byte_parity": (go_root["returncode"] == rust_root["returncode"]
                                           and go_root["stdout"] == rust_root["stdout"]
                                           and go_root["stderr"] == rust_root["stderr"]),
             "go_only_root_commands": sorted(go_commands - advertised),
             "rust_only_root_commands": sorted(advertised - go_commands),
             "go": output_record(go_root), "rust": output_record(rust_root)}]
    full_root_match = (go_root["returncode"] == rust_root["returncode"]
                       and go_root["stdout"] == rust_root["stdout"]
                       and go_root["stderr"] == rust_root["stderr"])
    rows.append({"case": "CLI-001-full-root", "argv": ["--help"], "matched": full_root_match,
                 "criterion": "byte-identical complete Go/Rust root help including command inventory",
                 "go_only_root_commands": sorted(go_commands - advertised),
                 "rust_only_root_commands": sorted(advertised - go_commands),
                 "go": output_record(go_root), "rust": output_record(rust_root)})
    go_paths = [
        ["a11y"], ["batch"], ["config"], ["config", "show"], ["doctor"], ["dialog"], ["dialog", "accept"], ["screenshot"],
        ["dialog", "auto"], ["dialog", "dismiss"], ["dialog", "status"], ["downloads"],
        ["tab"], ["tab", "list"], ["tab", "new"], ["tab", "switch"], ["tab", "close"],
        ["tab", "window"], ["tab", "window", "window"],
        ["session"], ["session", "id"], ["session", "list"], ["session", "info"],
        ["cache", "get"],
        ["check"], ["dblclick"], ["focus"], ["hover"], ["scroll"], ["select"], ["uncheck"], ["scrollintoview"],
        ["frame", "tree"], ["set", "offline"], ["eval"], ["flow", "list"],
        ["flow", "run"], ["flow", "validate"], ["mcp"], ["profiles"], ["state"],
        ["state", "clean"], ["state", "clear"], ["state", "key"], ["state", "key", "init"],
        ["state", "list"], ["state", "load"], ["state", "save"], ["state", "show"],
        ["storage"], ["storage", "clear"], ["storage", "get"], ["storage", "set"],
        ["policy"], ["policy", "explain"], ["trace", "export"], ["trace", "replay"], ["diff", "snapshot"], ["diff", "url"],
        ["journal"], ["journal", "tail"], ["journal", "show"],
        ["cookies"], ["cookies", "list"], ["cookies", "clear"], ["cookies", "set"], ["help"], ["upload"], ["version"],
    ]
    for path in go_paths:
        argv = [*path, "--help"]
        go_result = run_process(go, argv, env)
        rust_result = run_process(rust, argv, env)
        equal = all(go_result.get(key) == rust_result.get(key)
                    for key in ("returncode", "stdout", "stderr"))
        rows.append({"case": "CLI-001-supported-byte", "argv": argv, "matched": equal,
                     "criterion": "byte-identical Go help for a Rust-implemented command",
                     "go": output_record(go_result), "rust": output_record(rust_result)})
    for argv in (["flow", "--help"], ["flow", "validate", "--help"],
                 ["state", "key", "init", "--help"], ["tools", "list", "--help"],
                 ["config", "show", "--help"]):
        result = run_process(rust, list(argv), env)
        help_text = result["stdout"].decode("utf-8", "replace")
        okay = result["returncode"] == 0 and "Usage:" in help_text and not result["stderr"]
        rows.append({"case": "CLI-001-supported", "argv": list(argv), "matched": okay,
                     "criterion": "implemented command help exits successfully with usage",
                     "rust": output_record(result)})
    return rows


class UnixDaemonStub:
    def __init__(self, path: Path, *, status_probe: bool, request_count: int | None = None):
        self.path = path
        self.frame: dict[str, Any] | None = None
        self.frames: list[dict[str, Any]] = []
        self.error: str | None = None
        self.status_probe = status_probe
        self.request_count = request_count
        self.thread: threading.Thread | None = None
        self.listener: socket.socket | None = None

    def __enter__(self) -> "UnixDaemonStub":
        if not hasattr(socket, "AF_UNIX"):
            raise RuntimeError("this Python runtime does not expose Unix-domain sockets")
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
            request_count = self.request_count if self.request_count is not None else (2 if self.status_probe else 1)
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
                    self.frames.append(frame)
                    if request_index == 0:
                        self.frame = frame
                    response = None
                    if frame.get("cmd") == "daemon.status":
                        data = {"session": frame.get("session", "default")}
                    elif frame.get("cmd") in ("open", "goto"):
                        args = frame.get("args") or {}
                        data = {
                            "action": frame["cmd"], "url": args.get("url", ""), "http_status": 200,
                        }
                    elif frame.get("cmd") == "auth.login":
                        data = {"status": "logged_in", "url": "https://fixture.invalid",
                                "username_set": True, "password_set": True}
                    elif frame.get("cmd") == "trace.replay":
                        steps = (frame.get("args") or {}).get("steps", [])
                        if not steps:
                            response = {"success": False, "error": {
                                "code": "operation_failed",
                                "message": "trace contains no replayable steps",
                            }}
                        else:
                            data = trace_replay_result(steps)
                    elif frame.get("cmd") == "storage.list":
                        args = frame.get("args") or {}
                        data = {"origin": "https://fixture.invalid", "kind": args.get("kind", ""),
                                "items": {"alpha": "one", "beta": "two"}}
                    elif frame.get("cmd") == "storage.set":
                        data = {"set": (frame.get("args") or {}).get("key", "")}
                    elif frame.get("cmd") == "storage.clear":
                        data = {"cleared": (frame.get("args") or {}).get("kind", "")}
                    elif frame.get("cmd") == "cookies.list":
                        data = {"origin": "https://fixture.invalid", "cookies": [{
                            "name": "sid", "value": "0123456789abcdef", "domain": "fixture.invalid",
                            "path": "/", "expires": -1, "size": 20, "http_only": True,
                            "secure": True, "session": True, "same_site": "Lax",
                        }]}
                    elif frame.get("cmd") == "cookies.clear":
                        data = {"cleared": (frame.get("args") or {}).get("name", "")}
                    elif frame.get("cmd") == "cookies.set":
                        data = {"set": ((frame.get("args") or {}).get("cookie") or {}).get("name", "")}
                    elif frame.get("cmd") == "console.list":
                        data = {"entries": [{"type": "log", "text": "fixture console"}], "count": 1}
                    elif frame.get("cmd") == "errors.list":
                        data = {"entries": [{"text": "fixture exception", "stacktrace": ["fixture.js:1"]}], "count": 1}
                    elif frame.get("cmd") in ("console.clear", "errors.clear"):
                        data = {"cleared": True}
                    elif frame.get("cmd") == "upload":
                        data = {"uploaded": (frame.get("args") or {}).get("files", [])}
                    elif frame.get("cmd") == "read":
                        url = (frame.get("args") or {}).get("url", "")
                        data = {"title": "Fixture", "html": f"<p>{url.rsplit('/', 1)[-1]}</p>"}
                    elif frame.get("cmd") == "policy.explain":
                        data = {"explanation": "fixture policy explanation", "source": "built-in",
                                "decider": "policy", "guard_active": False}
                    elif frame.get("cmd") in ("journal.tail", "journal.show"):
                        args = frame.get("args") or {}
                        data = {"schema_version": 1, "session": args.get("session", "default"), "entries": [{
                            "schema_version": 1, "timestamp": "2026-09-25T12:00:00Z",
                            "session": args.get("session", "default"), "command": "open",
                            "args": {"url": "https://fixture.invalid/trace?a=one&b=two"},
                            "risk_class": "read", "decider": "policy", "result": "ok",
                        }]}
                    elif frame.get("cmd") == "session.list":
                        data = {"schema_version": 1, "sessions": []}
                    elif frame.get("cmd") == "session.info":
                        data = {"name": frame.get("session", "default"), "active_tabs": 0}
                    elif frame.get("cmd") == "a11y":
                        data = {"nodes": []}
                    elif frame.get("cmd") == "network.capture":
                        data = {"started": True}
                    elif frame.get("cmd") == "download.setdir":
                        data = {"download_dir": (frame.get("args") or {}).get("dir", "")}
                    elif frame.get("cmd") == "downloads.list":
                        data = {"downloads": [{
                            "state": "completed", "filename": "report.csv",
                            "url": "https://fixture.invalid/report.csv", "sha256": "a" * 64,
                        }], "count": 1}
                    elif frame.get("cmd") == "network.request":
                        data = {"request": {
                            "id": ((frame.get("args") or {}).get("id", "fixture-1")),
                            "url": "https://fixture.invalid/data.json",
                            "method": "GET", "type": "xhr", "status": 200,
                            "started_at": "2026-01-01T00:00:00Z", "finished": True,
                            "mime_type": "application/json",
                        }}
                    elif frame.get("cmd") == "network.requests":
                        data = {"requests": [
                            {"id": "r1", "method": "POST", "url": "https://fixture.invalid/api", "type": "XHR", "status": 201},
                            {"id": "r2", "method": "GET", "url": "https://fixture.invalid/app.js", "type": "Script", "status": 200},
                        ], "count": 2}
                    elif frame.get("cmd") == "snapshot":
                        data = {"tree": "shared\nafter\n", "refs": {}}
                    elif frame.get("cmd") in ("tabs.list", "tab.list"):
                        data = {"active": "t1", "tabs": [
                            {"id": "t1", "label": "research",
                             "url": "https://fixture.invalid/", "active": True},
                            {"id": "t2", "url": "about:blank", "active": False},
                        ]}
                    else:
                        args = frame.get("args") or {}
                        data = {"url": args.get("url", ""), "value": 2,
                                "expression": args.get("expression", "")}
                    if response is None:
                        args = frame.get("args") or {}
                        if frame.get("cmd") in ("open", "goto") and args.get("url", "").endswith("/failed"):
                            response = {"success": False, "error": {
                                "code": "operation_failed", "message": "fixture navigation failed",
                            }}
                        else:
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


class WindowsNamedPipeStub:
    """Bounded byte-stream stub for the production Windows daemon pipe."""

    PIPE_PREFIX = r"\\.\pipe\symbrowse-"
    PIPE_ACCESS_DUPLEX = 0x00000003
    PIPE_TYPE_BYTE = 0x00000000
    PIPE_READMODE_BYTE = 0x00000000
    PIPE_WAIT = 0x00000000
    ERROR_PIPE_CONNECTED = 535
    ERROR_PIPE_BUSY = 231
    INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
    GENERIC_READ = 0x80000000
    GENERIC_WRITE = 0x40000000
    OPEN_EXISTING = 3
    FILE_ATTRIBUTE_NORMAL = 0x00000080
    SHUTDOWN_FRAME = b'{"_harness_shutdown":true}\n'

    def __init__(self, *, session: str = SESSION, status_probe: bool, request_count: int | None = None):
        self.session = session
        self.pipe = self.PIPE_PREFIX + session
        self.frame: dict[str, Any] | None = None
        self.frames: list[dict[str, Any]] = []
        self.error: str | None = None
        self.status_probe = status_probe
        self.request_count = request_count
        self.flushed_responses = 0
        self.thread: threading.Thread | None = None
        self.ready = threading.Event()
        self.pipe_ready = threading.Event()
        self.stopping = threading.Event()
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.CreateNamedPipeW.argtypes = [
            wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD,
            wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID,
        ]
        self.kernel.CreateNamedPipeW.restype = wintypes.HANDLE
        self.kernel.ConnectNamedPipe.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
        self.kernel.ConnectNamedPipe.restype = wintypes.BOOL
        self.kernel.ReadFile.argtypes = [
            wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD,
            ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID,
        ]
        self.kernel.ReadFile.restype = wintypes.BOOL
        self.kernel.WriteFile.argtypes = [
            wintypes.HANDLE, wintypes.LPCVOID, wintypes.DWORD,
            ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID,
        ]
        self.kernel.WriteFile.restype = wintypes.BOOL
        self.kernel.FlushFileBuffers.argtypes = [wintypes.HANDLE]
        self.kernel.FlushFileBuffers.restype = wintypes.BOOL
        self.kernel.DisconnectNamedPipe.argtypes = [wintypes.HANDLE]
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.CreateFileW.argtypes = [
            wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID,
            wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE,
        ]
        self.kernel.CreateFileW.restype = wintypes.HANDLE
        self.kernel.WaitNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD]
        self.kernel.WaitNamedPipeW.restype = wintypes.BOOL

    def __enter__(self) -> "WindowsNamedPipeStub":
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()
        if not self.ready.wait(timeout=5):
            error = RuntimeError(self.error or "Windows daemon test pipe was not ready")
            self.__exit__()
            raise error
        return self

    def _win_error(self, operation: str) -> OSError:
        return ctypes.WinError(ctypes.get_last_error(), operation)

    def _serve(self) -> None:
        try:
            request_count = self.request_count if self.request_count is not None else (2 if self.status_probe else 1)
            for request_index in range(request_count):
                if self.stopping.is_set():
                    break
                handle = self.kernel.CreateNamedPipeW(
                    self.pipe, self.PIPE_ACCESS_DUPLEX,
                    self.PIPE_TYPE_BYTE | self.PIPE_READMODE_BYTE | self.PIPE_WAIT,
                    1, MAX_CAPTURE_BYTES, MAX_CAPTURE_BYTES, 5000, None,
                )
                if handle == self.INVALID_HANDLE_VALUE:
                    raise self._win_error("CreateNamedPipeW")
                self.pipe_ready.set()
                if request_index == 0:
                    self.ready.set()
                try:
                    if not self.kernel.ConnectNamedPipe(handle, None):
                        if ctypes.get_last_error() != self.ERROR_PIPE_CONNECTED:
                            raise self._win_error("ConnectNamedPipe")
                    raw = bytearray()
                    while b"\n" not in raw:
                        chunk = ctypes.create_string_buffer(65536)
                        read = wintypes.DWORD()
                        if not self.kernel.ReadFile(handle, chunk, len(chunk), ctypes.byref(read), None):
                            raise self._win_error("ReadFile")
                        if read.value == 0:
                            break
                        raw.extend(chunk.raw[:read.value])
                        if len(raw) > MAX_CAPTURE_BYTES:
                            raise RuntimeError("daemon request exceeded the 1 MiB bound")
                    if bytes(raw).split(b"\n", 1)[0] == self.SHUTDOWN_FRAME.rstrip(b"\n"):
                        break
                    frame = json.loads(bytes(raw).split(b"\n", 1)[0])
                    self.frames.append(frame)
                    if request_index == 0:
                        self.frame = frame
                    response_error = None
                    if frame.get("cmd") == "daemon.status":
                        data = {"session": frame.get("session", SESSION)}
                    elif frame.get("cmd") in ("open", "goto"):
                        args = frame.get("args") or {}
                        data = {
                            "action": frame["cmd"], "url": args.get("url", ""), "http_status": 200,
                        }
                    elif frame.get("cmd") == "auth.login":
                        data = {"status": "logged_in", "url": "https://fixture.invalid",
                                "username_set": True, "password_set": True}
                    elif frame.get("cmd") == "trace.replay":
                        steps = (frame.get("args") or {}).get("steps", [])
                        if not steps:
                            response_error = {"code": "operation_failed",
                                              "message": "trace contains no replayable steps"}
                        else:
                            data = trace_replay_result(steps)
                    elif frame.get("cmd") == "storage.list":
                        args = frame.get("args") or {}
                        data = {"origin": "https://fixture.invalid", "kind": args.get("kind", ""),
                                "items": {"alpha": "one", "beta": "two"}}
                    elif frame.get("cmd") == "storage.set":
                        data = {"set": (frame.get("args") or {}).get("key", "")}
                    elif frame.get("cmd") == "storage.clear":
                        data = {"cleared": (frame.get("args") or {}).get("kind", "")}
                    elif frame.get("cmd") == "cookies.list":
                        data = {"origin": "https://fixture.invalid", "cookies": [{
                            "name": "sid", "value": "0123456789abcdef", "domain": "fixture.invalid",
                            "path": "/", "expires": -1, "size": 20, "http_only": True,
                            "secure": True, "session": True, "same_site": "Lax",
                        }]}
                    elif frame.get("cmd") == "cookies.clear":
                        data = {"cleared": (frame.get("args") or {}).get("name", "")}
                    elif frame.get("cmd") == "cookies.set":
                        data = {"set": ((frame.get("args") or {}).get("cookie") or {}).get("name", "")}
                    elif frame.get("cmd") == "console.list":
                        data = {"entries": [{"type": "log", "text": "fixture console"}], "count": 1}
                    elif frame.get("cmd") == "errors.list":
                        data = {"entries": [{"text": "fixture exception", "stacktrace": ["fixture.js:1"]}], "count": 1}
                    elif frame.get("cmd") in ("console.clear", "errors.clear"):
                        data = {"cleared": True}
                    elif frame.get("cmd") == "upload":
                        data = {"uploaded": (frame.get("args") or {}).get("files", [])}
                    elif frame.get("cmd") == "read":
                        url = (frame.get("args") or {}).get("url", "")
                        data = {"title": "Fixture", "html": f"<p>{url.rsplit('/', 1)[-1]}</p>"}
                    elif frame.get("cmd") == "policy.explain":
                        data = {"explanation": "fixture policy explanation", "source": "built-in",
                                "decider": "policy", "guard_active": False}
                    elif frame.get("cmd") in ("journal.tail", "journal.show"):
                        args = frame.get("args") or {}
                        data = {"schema_version": 1, "session": args.get("session", "default"), "entries": [{
                            "schema_version": 1, "timestamp": "2026-09-25T12:00:00Z",
                            "session": args.get("session", "default"), "command": "open",
                            "args": {"url": "https://fixture.invalid/trace?a=one&b=two"},
                            "risk_class": "read", "decider": "policy", "result": "ok",
                        }]}
                    elif frame.get("cmd") == "session.list":
                        data = {"schema_version": 1, "sessions": []}
                    elif frame.get("cmd") == "session.info":
                        data = {"name": frame.get("session", SESSION), "active_tabs": 0}
                    elif frame.get("cmd") == "a11y":
                        data = {"nodes": []}
                    elif frame.get("cmd") == "network.capture":
                        data = {"started": True}
                    elif frame.get("cmd") == "download.setdir":
                        data = {"download_dir": (frame.get("args") or {}).get("dir", "")}
                    elif frame.get("cmd") == "downloads.list":
                        data = {"downloads": [{
                            "state": "completed", "filename": "report.csv",
                            "url": "https://fixture.invalid/report.csv", "sha256": "a" * 64,
                        }], "count": 1}
                    elif frame.get("cmd") == "network.request":
                        data = {"request": {
                            "id": ((frame.get("args") or {}).get("id", "fixture-1")),
                            "url": "https://fixture.invalid/data.json",
                            "method": "GET", "type": "xhr", "status": 200,
                            "started_at": "2026-01-01T00:00:00Z", "finished": True,
                            "mime_type": "application/json",
                        }}
                    elif frame.get("cmd") == "network.requests":
                        data = {"requests": [
                            {"id": "r1", "method": "POST", "url": "https://fixture.invalid/api", "type": "XHR", "status": 201},
                            {"id": "r2", "method": "GET", "url": "https://fixture.invalid/app.js", "type": "Script", "status": 200},
                        ], "count": 2}
                    elif frame.get("cmd") == "snapshot":
                        data = {"tree": "shared\nafter\n", "refs": {}}
                    elif frame.get("cmd") in ("tabs.list", "tab.list"):
                        data = {"active": "t1", "tabs": [
                            {"id": "t1", "label": "research",
                             "url": "https://fixture.invalid/", "active": True},
                            {"id": "t2", "url": "about:blank", "active": False},
                        ]}
                    else:
                        args = frame.get("args") or {}
                        data = {"url": args.get("url", ""), "value": 2,
                                "expression": args.get("expression", "")}
                    args = frame.get("args") or {}
                    if response_error is None and frame.get("cmd") in ("open", "goto") and args.get("url", "").endswith("/failed"):
                        response_error = {"code": "operation_failed",
                                          "message": "fixture navigation failed"}
                    response_payload = ({"success": False, "error": response_error}
                                        if response_error is not None else
                                        {"success": True, "data": data, "warnings": []})
                    response = json.dumps(response_payload, separators=(",", ":")).encode() + b"\n"
                    written = wintypes.DWORD()
                    buffer = ctypes.create_string_buffer(response, len(response))
                    if not self.kernel.WriteFile(handle, buffer, len(response), ctypes.byref(written), None):
                        raise self._win_error("WriteFile")
                    if written.value != len(response):
                        raise RuntimeError("short write to Windows daemon pipe")
                    if not self.kernel.FlushFileBuffers(handle):
                        raise self._win_error("FlushFileBuffers(response)")
                    self.flushed_responses += 1
                finally:
                    self.kernel.DisconnectNamedPipe(handle)
                    self.kernel.CloseHandle(handle)
                    self.pipe_ready.clear()
        except Exception as error:  # retained in the bounded report
            if not self.stopping.is_set():
                self.error = str(error)
            self.ready.set()

    def __exit__(self, *_: object) -> None:
        if self.thread is None:
            return
        if self.thread.is_alive():
            self.stopping.set()
            deadline = time.monotonic() + 3
            while self.thread.is_alive() and time.monotonic() < deadline:
                if not self.pipe_ready.wait(timeout=0.05):
                    continue
                handle = self.kernel.CreateFileW(
                    self.pipe, self.GENERIC_READ | self.GENERIC_WRITE, 0, None,
                    self.OPEN_EXISTING, self.FILE_ATTRIBUTE_NORMAL, None,
                )
                if handle == self.INVALID_HANDLE_VALUE:
                    last_error = ctypes.get_last_error()
                    if last_error == self.ERROR_PIPE_BUSY:
                        self.kernel.WaitNamedPipeW(self.pipe, 100)
                    else:
                        self.error = str(ctypes.WinError(last_error, "CreateFileW(shutdown)"))
                        break
                    continue
                try:
                    written = wintypes.DWORD()
                    payload = ctypes.create_string_buffer(self.SHUTDOWN_FRAME, len(self.SHUTDOWN_FRAME))
                    if not self.kernel.WriteFile(handle, payload, len(self.SHUTDOWN_FRAME), ctypes.byref(written), None):
                        self.error = str(self._win_error("WriteFile(shutdown)"))
                    elif written.value != len(self.SHUTDOWN_FRAME):
                        self.error = "short write to Windows daemon shutdown pipe"
                finally:
                    self.kernel.CloseHandle(handle)
                break
        self.thread.join(timeout=3)
        if self.thread.is_alive() and self.error is None:
            self.error = "Windows daemon stub thread did not stop after shutdown frame"


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


def socket_path(env: dict[str, str], session: str = SESSION) -> Path:
    if sys.platform == "darwin":
        return Path(env["HOME"]) / "Library/Caches/symbrowse/run" / f"{session}.sock"
    return Path(env["XDG_RUNTIME_DIR"]) / "symbrowse" / f"{session}.sock"


def session_from_argv(argv: list[str]) -> str:
    for index, value in enumerate(argv):
        if value == "--session" and index + 1 < len(argv):
            return argv[index + 1]
        if value.startswith("--session="):
            return value[len("--session="):]
    return SESSION


def compare_processes(go: Path, rust: Path, argv: list[str], stdin: bytes,
                      env: dict[str, str], *, stub: bool) -> dict[str, Any]:
    go_frame = rust_frame = None
    session = session_from_argv(argv)
    if stub:
        try:
            has_a11y_url = (argv[:1] == ["a11y"] and any(
                value.startswith(("http://", "https://")) for value in argv[1:]))
            is_diff_url = argv[:2] == ["diff", "url"]
            is_network_requests = argv[:2] == ["network", "requests"]
            is_downloads = argv[:1] == ["downloads"]
            download_dir = None
            if is_downloads:
                for index, value in enumerate(argv):
                    if value == "--dir" and index + 1 < len(argv):
                        download_dir = argv[index + 1]
                    elif value.startswith("--dir="):
                        download_dir = value[6:]
            go_count = 2 if has_a11y_url or is_diff_url or (is_downloads and download_dir) else 1
            # Client.request verifies daemon.status after each command frame.
            # diff url performs two reads, so the Rust stub must answer four
            # connections while the Go client sends only its two reads.
            rust_count = (4 if download_dir else 2) if is_downloads else (
                4 if has_a11y_url or is_diff_url or is_network_requests else 2
            )
            if os.name == "nt":
                with WindowsNamedPipeStub(session=session, status_probe=False, request_count=go_count) as go_stub:
                    go_result = run_process(go, argv, env, stdin)
                go_frame = go_stub.frame
                go_frames = go_stub.frames
                go_error = go_stub.error
                go_flushed = go_stub.flushed_responses
                with WindowsNamedPipeStub(session=session, status_probe=True, request_count=rust_count) as rust_stub:
                    rust_result = run_process(rust, argv, env, stdin)
                rust_frame = rust_stub.frame
                rust_frames = rust_stub.frames
                rust_error = rust_stub.error
                rust_flushed = rust_stub.flushed_responses
            else:
                go_path = socket_path(env, session)
                with UnixDaemonStub(go_path, status_probe=False, request_count=go_count) as go_stub:
                    go_result = run_process(go, argv, env, stdin)
                go_frame = go_stub.frame
                go_frames = go_stub.frames
                go_error = go_stub.error
                go_flushed = rust_flushed = None
                rust_path = socket_path(env, session)
                with UnixDaemonStub(rust_path, status_probe=True, request_count=rust_count) as rust_stub:
                    rust_result = run_process(rust, argv, env, stdin)
                rust_frame = rust_stub.frame
                rust_frames = rust_stub.frames
                rust_error = rust_stub.error
        except (OSError, RuntimeError) as error:
            return {"case": "harness_error", "argv": argv,
                    "reason": f"could not run bounded daemon stub: {error}"}
    else:
        go_result = run_process(go, argv, env, stdin)
        rust_result = run_process(rust, argv, env, stdin)
        go_error = rust_error = None
    matched = all(go_result.get(k) == rust_result.get(k) for k in ("returncode", "stdout", "stderr"))
    row = {"argv": argv, "matched": matched, "go": output_record(go_result),
           "rust": output_record(rust_result)}
    if argv in (
        ["snapshot", "--session", "fixture", "--output=yaml"],
        ["cookies", "list", "--session", "fixture", "--output=yaml"],
    ):
        # Keep complete, disposable-fixture YAML bytes in the artifact so
        # ordering and multiline scalar differences are inspectable.
        row["go_stdout_base64"] = base64.b64encode(go_result["stdout"]).decode("ascii")
        row["rust_stdout_base64"] = base64.b64encode(rust_result["stdout"]).decode("ascii")
    if stub:
        def payload(frame: dict[str, Any] | None) -> dict[str, Any] | None:
            if frame is None:
                return None
            result = {key: frame.get(key) for key in ("cmd", "session", "args")}
            # Go omits nil RawMessage args for these zero-argument wrappers;
            # Rust sends an empty object because its daemon validates object args.
            if result["cmd"] in {"dialog.status", "dialog.dismiss", "tab.list", "window.new", "frame.tree"} and result["args"] is None:
                result["args"] = {}
            # The Rust runtime exposes Go's set.offline operation through its
            # existing network.offline route; the CLI payload remains identical.
            if result["cmd"] == "network.offline":
                result["cmd"] = "set.offline"
            return result

        def payload_raw(frame: dict[str, Any] | None) -> dict[str, Any] | None:
            if frame is None:
                return None
            return {key: frame.get(key) for key in ("cmd", "session", "args")}

        row.update({"daemon_frames_match": go_frame == rust_frame,
                    "daemon_payloads_raw_match": payload_raw(go_frame) == payload_raw(rust_frame),
                    "go_frame": go_frame, "rust_frame": rust_frame,
                    "go_stub_error": go_error, "rust_stub_error": rust_error})
        row["daemon_payloads_match"] = payload(go_frame) == payload(rust_frame)
        row["matched"] = bool(row["matched"] and row["daemon_payloads_match"] and not go_error and not rust_error)
        if argv[:1] in (["console"], ["errors"]) and argv[1:2] == ["list"]:
            expected_budget = None
            for index, value in enumerate(argv):
                if value == "--max-tokens" and index + 1 < len(argv):
                    parsed = int(argv[index + 1])
                    expected_budget = parsed if parsed > 0 else None
                elif value.startswith("--max-tokens="):
                    parsed = int(value.split("=", 1)[1])
                    expected_budget = parsed if parsed > 0 else None
            budget_match = (
                go_frame is not None and rust_frame is not None
                and go_frame.get("max_tokens") == expected_budget
                and rust_frame.get("max_tokens") == expected_budget
            )
            row["max_tokens_frame_match"] = budget_match
            row["matched"] = bool(row["matched"] and budget_match)
        if argv[:1] == ["downloads"]:
            go_actual = [payload_raw(frame) for frame in go_frames]
            rust_actual = [payload_raw(frame) for frame in rust_frames if frame.get("cmd") != "daemon.status"]
            expected = ([{"cmd": "download.setdir", "session": session,
                          "args": {"dir": download_dir}},
                         {"cmd": "downloads.list", "session": session, "args": None}]
                        if download_dir else
                        [{"cmd": "downloads.list", "session": session, "args": None}])
            sequence_match = go_actual == expected and rust_actual == expected
            row["downloads_sequence_match"] = sequence_match
            row["go_actual_frames"] = go_actual
            row["rust_actual_frames"] = rust_actual
            row["matched"] = bool(
                all(go_result.get(key) == rust_result.get(key)
                    for key in ("returncode", "stdout", "stderr"))
                and sequence_match and not go_error and not rust_error
            )
            if os.name == "nt":
                row["windows_pipe_response_flushes"] = {"go": go_flushed, "rust": rust_flushed}
                row["windows_pipe_flush_verified"] = bool(
                    go_flushed == go_count and rust_flushed == rust_count
                )
                row["matched"] = bool(row["matched"] and row["windows_pipe_flush_verified"])
        if os.name == "nt" and (argv[:1] == ["eval"] or argv[:2] == ["tab", "switch"]):
            row["windows_pipe_response_flushes"] = {"go": go_flushed, "rust": rust_flushed}
            row["windows_pipe_flush_verified"] = bool(
                go_flushed and rust_flushed and not go_error and not rust_error
            )
            row["matched"] = bool(row["matched"] and row["windows_pipe_flush_verified"])
        if argv[:2] == ["network", "requests"]:
            go_actual = [payload_raw(frame) for frame in go_frames if frame.get("cmd") != "daemon.status"]
            rust_actual = [payload_raw(frame) for frame in rust_frames if frame.get("cmd") != "daemon.status"]
            sequence_match = (
                len(go_actual) == 1 and go_actual[0] is not None
                and go_actual[0].get("cmd") == "network.requests"
                and len(rust_actual) == 2 and rust_actual[0] is not None and rust_actual[1] is not None
                and [frame.get("cmd") for frame in rust_actual] == ["network.capture", "network.requests"]
                and all(frame.get("session") == session for frame in go_actual + rust_actual if frame is not None)
                and all(frame.get("args") is None for frame in go_actual + rust_actual if frame is not None)
            )
            row["network_capture_sequence_match"] = sequence_match
            row["go_actual_frames"] = go_actual
            row["rust_actual_frames"] = rust_actual
            output_match = all(go_result.get(key) == rust_result.get(key)
                               for key in ("returncode", "stdout", "stderr"))
            row["matched"] = bool(output_match and sequence_match and not go_error and not rust_error)
        if argv[:2] == ["diff", "url"]:
            go_actual = [payload_raw(frame) for frame in go_frames if frame.get("cmd") != "daemon.status"]
            rust_actual = [payload_raw(frame) for frame in rust_frames if frame.get("cmd") != "daemon.status"]
            ordered_reads_match = (
                len(go_actual) == len(rust_actual) == 2
                and all(frame is not None and frame.get("cmd") == "read" for frame in go_actual)
                and go_actual == rust_actual
            )
            row["ordered_diff_reads_match"] = ordered_reads_match
            row["matched"] = bool(row["matched"] and ordered_reads_match)
        if argv[:1] == ["a11y"]:
            def actual_frames(frames: list[dict[str, Any]]) -> list[dict[str, Any]]:
                return [payload_raw(frame) for frame in frames if frame.get("cmd") != "daemon.status"]

            go_actual = actual_frames(go_frames)
            rust_actual = actual_frames(rust_frames)
            row["go_actual_frames"] = go_actual
            row["rust_actual_frames"] = rust_actual
            row["daemon_actual_frames_match"] = go_actual == rust_actual
            row["matched"] = bool(row["matched"] and row["daemon_actual_frames_match"])
        if argv[:1] == ["diff"]:
            def actual_frames(frames: list[dict[str, Any]]) -> list[dict[str, Any]]:
                return [payload_raw(frame) for frame in frames if frame.get("cmd") != "daemon.status"]

            go_actual = actual_frames(go_frames)
            rust_actual = actual_frames(rust_frames)
            row["go_actual_frames"] = go_actual
            row["rust_actual_frames"] = rust_actual
            row["daemon_actual_frames_match"] = go_actual == rust_actual
            row["matched"] = bool(
                go_result.get("returncode") == rust_result.get("returncode")
                and go_result.get("stdout") == rust_result.get("stdout")
                and go_result.get("stderr") == rust_result.get("stderr")
                and row["daemon_actual_frames_match"] and not go_error and not rust_error
            )
    return row


def compare_watch_signal(go: Path, rust: Path, env: dict[str, str], argv: list[str],
                         sig: signal.Signals) -> dict[str, Any]:
    if os.name != "posix":
        return {"case": "CLI-watch-signal", "argv": argv, "signal": sig.name,
                "matched": None, "skipped": "graceful signal delivery is tested on POSIX targets"}

    marker = b"2026-09-25T12:00:00Z\topen\tclass=read\tdecider=policy\tok\n"

    def run_one(binary: Path, *, status_probe: bool, request_count: int) -> dict[str, Any]:
        with UnixDaemonStub(socket_path(env, SESSION), status_probe=status_probe,
                            request_count=request_count) as stub:
            process = subprocess.Popen(
                [str(binary), *argv], stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
            )
            captured = bytearray()
            deadline = time.monotonic() + 8
            try:
                assert process.stdout is not None
                with selectors.DefaultSelector() as selector:
                    selector.register(process.stdout, selectors.EVENT_READ)
                    while marker not in captured and time.monotonic() < deadline:
                        events = selector.select(timeout=max(0, deadline - time.monotonic()))
                        if not events:
                            break
                        chunk = os.read(process.stdout.fileno(), 4096)
                        if not chunk:
                            break
                        captured.extend(chunk)
                if marker not in captured:
                    process.kill()
                    process.wait(timeout=3)
                    tail, error = process.communicate(timeout=3)
                    return {"error": "watch did not emit the fixture journal row before timeout",
                            "stdout": bytes(captured) + tail, "stderr": error,
                            "returncode": process.returncode, "frames": stub.frames,
                            "stub_error": stub.error}
                process.send_signal(sig)
                process.wait(timeout=5)
                tail, error = process.communicate(timeout=3)
                return {"returncode": process.returncode, "stdout": bytes(captured) + tail,
                        "stderr": error, "frames": stub.frames, "stub_error": stub.error}
            except (OSError, subprocess.TimeoutExpired) as error:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=3)
                tail, stderr = process.communicate(timeout=3)
                return {"error": str(error), "returncode": process.returncode,
                        "stdout": bytes(captured) + tail, "stderr": stderr,
                        "frames": stub.frames, "stub_error": stub.error}

    go_result = run_one(go, status_probe=False, request_count=1)
    rust_result = run_one(rust, status_probe=True, request_count=2)
    expected_go = ["journal.show"]
    expected_rust = ["journal.show", "daemon.status"]

    def actual_commands(result: dict[str, Any]) -> list[str]:
        return [frame.get("cmd", "") for frame in result.get("frames", [])]

    go_frame = go_result.get("frames", [{}])[0]
    rust_frame = rust_result.get("frames", [{}])[0]
    metadata_match = all(
        frame.get(key) == expected
        for frame, expected in ((go_frame, "1"), (rust_frame, "1"))
        for key in ("request_id",)
    ) and all(
        frame.get(key) == expected
        for frame, expected in ((go_frame, "cli"), (rust_frame, "cli"))
        for key in ("retrieval_surface",)
    )
    frames_match = (actual_commands(go_result) == expected_go
                    and actual_commands(rust_result) == expected_rust and metadata_match)
    matched = (
        go_result.get("returncode") == rust_result.get("returncode") == 0
        and go_result.get("stdout") == rust_result.get("stdout")
        and go_result.get("stderr") == rust_result.get("stderr")
        and frames_match
        and not go_result.get("stub_error")
        and not rust_result.get("stub_error")
        and not go_result.get("error")
        and not rust_result.get("error")
    )
    return {"case": "CLI-watch-signal", "argv": argv, "signal": sig.name,
            "matched": matched, "bounded_shutdown_seconds": 5,
            "go": output_record(go_result), "rust": output_record(rust_result),
            "go_frames": go_result.get("frames"), "rust_frames": rust_result.get("frames"),
            "cli_frame_metadata_match": metadata_match,
            "go_stub_error": go_result.get("stub_error"),
            "rust_stub_error": rust_result.get("stub_error"),
            "go_error": go_result.get("error"), "rust_error": rust_result.get("error")}


def run_watch_cases(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    if os.name == "nt":
        return [
            compare_watch_windows_output(go, rust, env, argv)
            for argv in (["watch"], ["watch", "--json"])
        ] + [{"case": "CLI-watch-signal", "argv": ["watch"], "signal": "CTRL_C/SIGTERM",
              "matched": None,
              "evidence_gap": "the harness cannot safely broadcast CTRL_C_EVENT to only the child process; Windows graceful shutdown remains unverified"}]
    return [
        compare_watch_signal(go, rust, env, argv, sig)
        for argv in (["watch"], ["watch", "--json"])
        for sig in (signal.SIGINT, signal.SIGTERM)
    ]


def compare_watch_windows_output(go: Path, rust: Path, env: dict[str, str],
                                 argv: list[str]) -> dict[str, Any]:
    marker = b"2026-09-25T12:00:00Z\topen\tclass=read\tdecider=policy\tok\n"

    def run_one(binary: Path, *, status_probe: bool, request_count: int) -> dict[str, Any]:
        with WindowsNamedPipeStub(status_probe=status_probe, request_count=request_count) as stub:
            process = subprocess.Popen(
                [str(binary), *argv], stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                creationflags=subprocess.CREATE_NEW_PROCESS_GROUP,
            )
            captured = bytearray()
            emitted = threading.Event()

            def collect() -> None:
                assert process.stdout is not None
                for line in iter(process.stdout.readline, b""):
                    captured.extend(line)
                    if marker in captured:
                        emitted.set()

            reader = threading.Thread(target=collect, daemon=True)
            reader.start()
            if not emitted.wait(timeout=8):
                process.kill()
                process.wait(timeout=3)
                reader.join(timeout=3)
                stderr = process.stderr.read() if process.stderr is not None else b""
                return {"error": "watch did not emit the fixture journal row before timeout",
                        "returncode": process.returncode, "stdout": bytes(captured),
                        "stderr": stderr, "frames": stub.frames, "stub_error": stub.error}
            process.terminate()
            process.wait(timeout=3)
            reader.join(timeout=3)
            stderr = process.stderr.read() if process.stderr is not None else b""
            return {"returncode": process.returncode, "stdout": bytes(captured),
                    "stderr": stderr, "frames": stub.frames, "stub_error": stub.error}

    go_result = run_one(go, status_probe=False, request_count=1)
    rust_result = run_one(rust, status_probe=True, request_count=2)
    go_frame = go_result.get("frames", [{}])[0]
    rust_frame = rust_result.get("frames", [{}])[0]
    matched = (
        marker in go_result.get("stdout", b"") and marker in rust_result.get("stdout", b"")
        and go_result.get("stdout") == rust_result.get("stdout")
        and go_frame.get("cmd") == rust_frame.get("cmd") == "journal.show"
        and go_frame.get("request_id") == rust_frame.get("request_id") == "1"
        and go_frame.get("retrieval_surface") == rust_frame.get("retrieval_surface") == "cli"
        and not go_result.get("error") and not rust_result.get("error")
        and not go_result.get("stub_error") and not rust_result.get("stub_error")
    )
    return {"case": "CLI-watch-output-windows", "argv": argv, "matched": matched,
            "criterion": "text/JSON-flag output and journal request match before bounded process termination",
            "graceful_signal_evidence": "not covered by this output case",
            "go": output_record(go_result), "rust": output_record(rust_result),
            "go_frames": go_result.get("frames"), "rust_frames": rust_result.get("frames"),
            "go_stub_error": go_result.get("stub_error"),
            "rust_stub_error": rust_result.get("stub_error"),
            "go_error": go_result.get("error"), "rust_error": rust_result.get("error")}


def run_fixed_cases(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    cache_root = Path(env["SYMBROWSE_CACHE_DIR"])
    output_id = "out_0123456789ab"
    output_root = cache_root / "out"
    output_root.mkdir(parents=True, exist_ok=True)
    (output_root / f"{output_id}.json").write_bytes(b"first\nsecond\nthird")
    expires = (datetime.now(timezone.utc) + timedelta(hours=1)).isoformat().replace("+00:00", "Z")
    created = (datetime.now(timezone.utc) - timedelta(minutes=1)).isoformat().replace("+00:00", "Z")
    (output_root / f"{output_id}.meta.json").write_text(json.dumps({
        "id": output_id, "created_at": created, "expires_at": expires,
    }))
    fetch_key = "a" * 64
    fetch_root = cache_root / "fetch" / fetch_key[:2]
    fetch_root.mkdir(parents=True, exist_ok=True)
    (fetch_root / f"{fetch_key}.body").write_bytes(b"first\nsecond\nthird")
    (fetch_root / f"{fetch_key}.meta.json").write_text(json.dumps({
        "url": "https://fixture.invalid", "stored_at": created, "ttl": 86_400_000_000_000,
    }))
    cookie_jar = Path(env["HOME"]) / "fixture.cookies.txt"
    cookie_jar.write_text("# Netscape HTTP Cookie File\n.fixture.invalid\tTRUE\t/\tFALSE\t0\tjar_sid\tjar-value\n")
    cases = [
        ("CLI-002", ["cache", "get", output_id], b"", False),
        ("CLI-002", ["cache", "get", output_id, "--range=2-2", "--json"], b"", False),
        ("CLI-002", ["cache", "get", f"fetch:{fetch_key}", "--range=2-3", "--json"], b"", False),
        ("CLI-003", ["cache", "get", "fetch:bad-key", "--json"], b"", False),
        ("CLI-003", ["cache", "get", output_id, "--range=0-2", "--json"], b"", False),
        ("CLI-002", ["a11y", "--json"], b"", True),
        ("CLI-002", ["a11y", "--selector", ".main", "--tags", "wcag2a, ,wcag2aa", "https://fixture.invalid", "--json"], b"", True),
        ("CLI-003", ["a11y", "one", "two"], b"", False),
        ("CLI-002", ["screenshot"], b"", True),
        ("CLI-002", ["screenshot", "--json"], b"", True),
        ("CLI-002", ["screenshot", "fixture.jpg", "--full", "--selector", "#hero", "--format", "jpeg", "--quality", "80", "--screenshot-dir", "/tmp/screens", "--json"], b"", True),
        ("CLI-003", ["screenshot", "one", "two"], b"", False),
        ("CLI-002", ["goto", "https://fixture.invalid", "--json"], b"", True),
        ("CLI-002", ["open", "https://fixture.invalid", "--json"], b"", True),
        ("OUT-003", ["open", "https://fixture.invalid", "--session", "fixture"], b"", True),
        ("OUT-003", ["open", "https://fixture.invalid", "--session", "fixture", "--output=yaml"], b"", True),
        ("OUT-003", ["snapshot", "--session", "fixture"], b"", True),
        ("OUT-003", ["snapshot", "--session", "fixture", "--output=yaml"], b"", True),
        ("CLI-002", ["auth", "login", "fixture-entry", "--session", "fixture"], b"", True),
        ("CLI-002", ["auth", "login", "fixture-entry", "--url", "https://fixture.invalid/login", "--session", "fixture", "--json"], b"", True),
        ("CLI-003", ["auth", "login"], b"", False),
        ("CLI-003", ["auth", "login", "fixture-entry", "extra"], b"", False),
        ("OUT-003", ["tab", "list", "--session", "fixture"], b"", True),
        ("OUT-003", ["tab", "list", "--session", "fixture", "--output=yaml"], b"", True),
        ("OUT-003", ["cookies", "list", "--session", "fixture", "--output=yaml"], b"", True),
        ("CLI-002", ["console", "list", "--session", "fixture"], b"", True),
        ("CLI-002", ["console", "list", "--session", "fixture", "--json"], b"", True),
        ("CLI-002", ["console", "list", "--session", "fixture", "--max-tokens", "12", "--json"], b"", True),
        ("CLI-002", ["console", "list", "--session", "fixture", "--max-tokens=0", "--json"], b"", True),
        ("CLI-002", ["errors", "list", "--session", "fixture", "--max-tokens", "-1", "--json"], b"", True),
        ("CLI-002", ["console", "clear", "--session", "fixture"], b"", True),
        ("CLI-002", ["console", "clear", "--session", "fixture", "--json"], b"", True),
        ("CLI-002", ["errors", "list", "--session", "fixture"], b"", True),
        ("CLI-002", ["errors", "list", "--session", "fixture", "--json"], b"", True),
        ("CLI-002", ["errors", "clear", "--session", "fixture"], b"", True),
        ("CLI-002", ["errors", "clear", "--session", "fixture", "--json"], b"", True),
        ("CLI-002", ["journal", "tail", "--lines", "1", "--session", "fixture"], b"", True),
        ("CLI-002", ["journal", "show", "--session", "fixture", "--json"], b"", True),
        ("CLI-003", ["journal", "tail", "extra"], b"", False),
        ("CLI-002", ["policy", "explain", "snapshot", "--url", "https://fixture.invalid", "--mode", "tty"], b"", True),
        ("CLI-002", ["policy", "explain", "snapshot", "--url", "https://fixture.invalid", "--mode", "mcp", "--json"], b"", True),
        ("CLI-003", ["policy", "explain"], b"", False),
        ("CLI-003", ["policy", "explain", "snapshot", "extra"], b"", False),
        ("CLI-002", ["upload", "input[type=file]", "one.txt", "two.txt"], b"", True),
        ("CLI-002", ["upload", "@e2", "--json", "--", "-leading-dash.txt"], b"", True),
        ("CLI-002", ["network", "requests"], b"", True),
        ("CLI-002", ["network", "requests", "--filter", "API", "--type", "xhr", "--method", "post", "--status", "201", "--json"], b"", True),
        ("CLI-002", ["network", "requests", "--output=yaml"], b"", True),
        ("CLI-002", ["network", "request", "fixture-1"], b"", True),
        ("CLI-002", ["network", "request", "fixture-1", "--session", "fixture", "--json"], b"", True),
        ("OUT-003", ["network", "request", "fixture-1", "--session", "fixture", "--output=yaml"], b"", True),
        ("CLI-003", ["network", "request"], b"", False),
        ("CLI-003", ["network", "request", "one", "two"], b"", False),
        ("CLI-003", ["network", "requests", "extra"], b"", False),
        ("CLI-002", ["downloads", "--session", "fixture"], b"", True),
        ("CLI-002", ["downloads", "--session", "fixture", "--json"], b"", True),
        ("CLI-002", ["downloads", "--dir", "/tmp/symbrowse-downloads", "--session", "fixture"], b"", True),
        ("CLI-002", ["downloads", "--dir=/tmp/symbrowse-downloads", "--session=fixture", "--output=json"], b"", True),
        ("CLI-003", ["downloads", "extra"], b"", False),
        ("CLI-003", ["downloads", "--dir"], b"", False),
        ("CLI-003", ["upload"], b"", False),
        ("CLI-003", ["upload", "input[type=file]"], b"", False),
        ("CLI-003", ["state", "save"], b"", False),
        ("CLI-003", ["state", "save", "one", "extra"], b"", False),
        ("CLI-003", ["version", "extra"], b"", False),
        ("CLI-003", ["version", "--", "--json"], b"", False),
        ("CLI-003", ["--unknown", "version"], b"", False),
        ("CLI-003", ["version", "--unknown"], b"", False),
        ("CLI-003", ["config", "show", "--config-dir"], b"", False),
        ("CLI-003", ["mcp", "--engine"], b"", False),
        ("CLI-004", ["--json", "config", "show"], b"", False),
        ("CLI-004", ["config", "--json", "show"], b"", False),
        ("CLI-004", ["config", "show", "--json"], b"", False),
        ("CLI-004", ["config", "show", "--output", "json"], b"", False),
        ("CLI-004", ["config", "--json=false", "show"], b"", False),
        ("CLI-004", ["config", "show", "--json=true", "--json=false"], b"", False),
        ("CLI-004", ["config", "show", "--json=false", "--json"], b"", False),
        ("CLI-004", ["config", "--output=json", "show", "--json"], b"", False),
        ("CLI-004", ["config", "show", "--output", "yaml", "--output=text"], b"", False),
        ("CLI-004", ["config", "show", "--", "--json"], b"", False),
        ("CLI-004", ["version", "--json=false"], b"", False),
        ("CLI-004", ["version", "--json=true", "--json=false"], b"", False),
        ("CLI-004", ["version", "--json=false", "--output=yaml"], b"", False),
        ("CLI-004", ["flow", "--json", "list"], b"", False),
        ("CLI-004", ["flow", "--output", "yaml", "list"], b"", False),
        ("CLI-004", ["flow", "list", "--output=yaml"], b"", False),
        ("CLI-004", ["flow", "list", "--output=yaml", "--output=json"], b"", False),
        ("CLI-004", ["flow", "list", "--json=false"], b"", False),
        ("CLI-004", ["flow", "list", "--json=false", "--json"], b"", False),
        ("CLI-004", ["flow", "list", "--json", "--json=false"], b"", False),
        ("CLI-004", ["flow", "list", "--", "--json"], b"", False),
        ("CLI-004", ["--output", "yaml", "flow", "list"], b"", False),
        ("CLI-004", ["open", "https://fixture.invalid", "--json=false"], b"", True),
        ("CLI-004", ["open", "https://fixture.invalid", "--json=false", "--json"], b"", True),
        ("CLI-004", ["open", "https://fixture.invalid", "--", "--json"], b"", False),
        ("CLI-004", ["--json", "eval", "1+1"], b"", True),
        ("CLI-004", ["--output", "json", "eval", "1+1"], b"", True),
        ("CLI-004", ["eval", "--output", "json", "1+1"], b"", True),
        ("CLI-004", ["--output", "yaml", "eval", "1+1", "--json"], b"", True),
        ("CLI-005", ["eval", "1+1", "--json"], b"", True),
        ("CLI-005", ["eval", "1+1"], b"", True),
        ("CLI-005", ["eval", "ignored", "--stdin", "--json"], b"document.title", True),
        ("CLI-005", ["eval", "ignored", "--stdin=false", "--json"], b"document.title", True),
        ("CLI-005", ["eval", "ignored", "--stdin=true", "--json"], b"document.title", True),
        ("CLI-005", ["eval", "first", "second", "--json"], b"", True),
        ("CLI-005", ["eval", "1+1", "--stdin", "--stdin=false", "--json"], b"ignored", True),
        ("CLI-005", ["eval", "ZG9jdW1lbnQudGl0bGU=", "--base64", "--json"], b"", True),
        ("CLI-005", ["eval", "ZG9jdW1lbnQudGl0bGU=", "--base64", "--base64=false", "--json"], b"", True),
        ("CLI-005", ["eval", "ZG9jdW1ludQudGl0bGU=", "--base64=false", "--base64", "--json"], b"", True),
        ("CLI-005", ["eval", "1+1", "--base64=false", "--json"], b"", True),
        ("CLI-005", ["eval", "--base64=false", "--base64", "MSsy", "--json"], b"", True),
        ("CLI-005", ["eval", "MSsy", "-b=false", "--json"], b"", True),
        ("CLI-005", ["eval", "--stdin", "--base64", "--json"], b"MSsy", True),
        ("CLI-005", ["eval", "--stdin", "--stdin=false", "--base64", "MSsy", "--json"], b"", True),
        ("CLI-005", ["eval", "--base64", "--", "MSsy", "--json"], b"", True),
        ("CLI-005", ["eval", "--", "--stdin", "--base64", "--json"], b"", True),
        ("CLI-005", ["eval", "1+1", "--", "--base64", "--json"], b"", True),
        ("CLI-005", ["--output=yaml", "eval", "1+1", "--json=false"], b"", True),
        ("CLI-005", ["eval", "1+1", "--output=yaml", "--output=json"], b"", True),
        ("CLI-005", ["eval", "1+1", "extra", "--json"], b"", True),
        ("CLI-005", ["eval", "--json"], b"", False),
        ("CLI-005", ["eval", "!!!", "--base64", "--json"], b"", False),
        ("CLI-005", ["eval", "YQ==x", "--base64", "--json"], b"", False),
        ("CLI-005", ["eval", "YQ=\n", "--base64", "--json"], b"", False),
        ("CLI-005", ["eval", "AA==\nA", "--base64", "--json"], b"", False),
        ("CLI-005", ["eval", "A===", "--base64", "--json"], b"", False),
        ("CLI-005", ["eval", "AA=A", "--base64", "--json"], b"", False),
        ("CLI-005", ["eval", "AAA", "--base64", "--json"], b"", False),
        ("CLI-005", ["eval", "====", "--base64", "--json"], b"", False),
        ("CLI-005", ["eval", "--stdin", "--base64", "--json"], b"", True),
        ("CLI-005", ["eval", "AB==", "--base64", "--json"], b"", True),
        ("CLI-002", ["dialog", "status", "--json"], b"", True),
        ("CLI-002", ["dialog", "dismiss", "--output", "yaml"], b"", True),
        ("CLI-002", ["dialog", "accept"], b"", True),
        ("CLI-002", ["dialog", "accept", "prompt text", "--session", "default"], b"", True),
        ("CLI-002", ["dialog", "auto", "accept", "--session=default", "--output=json"], b"", True),
        ("CLI-002", ["dialog", "--session", "default", "status"], b"", True),
        ("CLI-002", ["storage", "get", "local", "beta"], b"", True),
        ("CLI-002", ["--output=json", "storage", "--session=default", "get", "session", "alpha"], b"", True),
        ("CLI-002", ["storage", "--session", "default", "get", "local", "--json"], b"", True),
        ("CLI-002", ["storage", "set", "session", "alpha", "value with spaces"], b"", True),
        ("CLI-002", ["--output=json", "storage", "--session=default", "set", "local", "quoted\"key", "line\nvalue"], b"", True),
        ("CLI-002", ["storage", "clear", "local", "--json"], b"", True),
        ("CLI-002", ["cookies", "list"], b"", True),
        ("CLI-002", ["cookies", "list", "--json"], b"", True),
        ("CLI-002", ["cookies", "list", "--reveal", "sid", "--json"], b"", True),
        ("CLI-002", ["cookies", "clear", "sid"], b"", True),
        ("CLI-002", ["cookies", "clear", "sid", "--url", "https://fixture.invalid/" , "--json"], b"", True),
        ("CLI-002", ["cookies", "set", "sid", "value", "--url", "https://fixture.invalid/", "--json"], b"", True),
        ("CLI-002", ["cookies", "set", "sid", "value", "--domain", ".fixture.invalid", "--path", "/", "--secure", "--http-only"], b"", True),
        ("CLI-002", ["cookies", "set", "--curl", str(cookie_jar)], b"", True),
        ("CLI-002", ["storage", "--session", "default", "clear", "session"], b"", True),
        ("CLI-002", ["session", "list", "--json"], b"", True),
        ("CLI-002", ["session", "--session", "default", "info", "--output=json"], b"", True),
        ("CLI-002", ["session", "id"], b"", False),
        ("CLI-002", ["session", "id", "--scope=repo", "--prefix=fixture", "--json"], b"", False),
        ("CLI-002", ["session", "id", "--scope=cwd", "--prefix=html<&", "--output=json"], b"", False),
        ("CLI-003", ["session", "--scope=repo", "id"], b"", False),
        ("CLI-003", ["session", "id", "--scope=invalid", "--json"], b"", False),
        ("CLI-003", ["storage", "set", "local", "key"], b"", False),
        ("CLI-003", ["storage", "clear"], b"", False),
        ("CLI-003", ["cookies", "list", "extra"], b"", False),
        ("CLI-003", ["cookies", "clear"], b"", False),
        ("CLI-003", ["cookies", "clear", "one", "two"], b"", False),
        ("CLI-003", ["cookies", "set", "only-one"], b"", False),
        ("CLI-003", ["dialog", "auto"], b"", False),
        ("CLI-003", ["dialog", "auto", "accept", "extra"], b"", False),
        ("CLI-003", ["dialog", "accept", "a", "b"], b"", False),
        ("CLI-003", ["dialog", "status", "extra"], b"", False),
        ("CLI-003", ["dialog", "dismiss", "extra"], b"", False),
        ("CLI-003", ["dialog", "--bad"], b"", False),
        ("CLI-003", ["dialog", "--session"], b"", False),
        ("CLI-003", ["dialog", "accept", "--text", "x"], b"", False),
        ("CLI-004", ["--json", "dialog", "status"], b"", True),
        ("CLI-004", ["dialog", "--json=false", "status", "--output=yaml"], b"", True),
        ("CLI-004", ["dialog", "status", "--", "--json"], b"", False),
        ("CLI-002", ["tab", "list", "--json"], b"", True),
        ("CLI-002", ["tab", "new", "https://fixture.invalid", "--label", "fixture", "--session", "default"], b"", True),
        ("CLI-002", ["tab", "switch", "t2", "--session=default", "--output=json"], b"", True),
        ("CLI-002", ["tab", "close", "t2"], b"", True),
        ("CLI-002", ["tab", "close"], b"", True),
        ("CLI-002", ["tab", "window", "window", "--json"], b"", True),
        ("CLI-002", ["tab", "--session", "default", "list"], b"", True),
        ("CLI-003", ["tab", "nope"], b"", False),
        ("CLI-003", ["tab", "list", "extra"], b"", False),
        ("CLI-003", ["tab", "new", "a", "b"], b"", False),
        ("CLI-003", ["tab", "switch"], b"", False),
        ("CLI-003", ["tab", "switch", "a", "b"], b"", False),
        ("CLI-003", ["tab", "close", "a", "b"], b"", False),
        ("CLI-003", ["tab", "window", "window", "extra"], b"", False),
        ("CLI-003", ["tab", "--bad"], b"", False),
        ("CLI-003", ["tab", "--session"], b"", False),
        ("CLI-003", ["tab", "list", "--label", "x"], b"", False),
        ("CLI-003", ["tab", "new", "--label"], b"", False),
        ("CLI-002", ["check", "#agree", "--json"], b"", True),
        ("CLI-002", ["dblclick", "button.submit"], b"", True),
        ("CLI-002", ["focus", "#search", "--session", "default"], b"", True),
        ("CLI-002", ["hover", "#menu"], b"", True),
        ("CLI-002", ["select", "#country", "DE"], b"", True),
        ("CLI-002", ["select", "#country"], b"", True),
        ("CLI-002", ["uncheck", "#newsletter"], b"", True),
        ("CLI-002", ["scrollintoview", "#target", "--output=json"], b"", True),
        ("CLI-002", ["scroll", "--output=json", "#target", "--", "-240"], b"", True),
        ("CLI-002", ["scroll", "#target"], b"", True),
        ("CLI-003", ["check"], b"", False),
        ("CLI-003", ["check", "#agree", "extra"], b"", False),
        ("CLI-003", ["select", "#country", "DE", "extra"], b"", False),
        ("CLI-003", ["scrollintoview", "#target", "extra"], b"", False),
        ("CLI-003", ["scroll", "#target", "bad-amount"], b"", False),
        ("CLI-003", ["scroll", "#target", "1", "extra"], b"", False),
        ("CLI-003", ["check", "--selector", "#agree"], b"", False),
        ("CLI-003", ["check", "--", "--selector"], b"", True),
        ("CLI-002", ["frame", "tree", "--json"], b"", True),
        ("CLI-002", ["frame", "--session", "default", "tree"], b"", True),
        ("CLI-003", ["frame", "tree", "extra"], b"", False),
        ("CLI-003", ["frame", "--session"], b"", False),
        ("CLI-002", ["set", "offline"], b"", True),
        ("CLI-002", ["set", "offline", "off", "--session", "default", "--json"], b"", True),
        ("CLI-004", ["--output=json", "set", "offline", "on", "--json=false", "--output=yaml"], b"", True),
        ("CLI-003", ["set", "offline", "maybe"], b"", False),
        ("CLI-003", ["set", "offline", "on", "off"], b"", False),
    ]
    comparisons = []
    for contract, argv, stdin, stub in cases:
        row = compare_processes(go, rust, argv, stdin, env, stub=stub)
        row["case"] = contract
        comparisons.append(row)
    comparisons.append(compare_trace_export(go, rust, env))
    comparisons.extend([
        compare_trace_replay(go, rust, env, []),
        compare_trace_replay(go, rust, env, ["--json"]),
        compare_trace_replay(go, rust, env, ["--output=yaml"]),
        compare_trace_replay_empty(go, rust, env, []),
        compare_trace_replay_empty(go, rust, env, ["--json"]),
        compare_trace_replay_empty(go, rust, env, ["--output=yaml"]),
        compare_trace_schema_error(go, rust, env, []),
        compare_trace_schema_error(go, rust, env, ["--json"]),
        compare_trace_schema_error(go, rust, env, ["--output=yaml"]),
    ])
    comparisons.extend([
        compare_diff_snapshot(go, rust, env, []),
        compare_diff_snapshot(go, rust, env, ["--json"]),
        compare_diff_url(go, rust, env),
    ])
    doctor_env = dict(env)
    isolated_path = Path(env["TMPDIR"]) / "doctor-empty-path"
    isolated_path.mkdir(exist_ok=True)
    doctor_env["PATH"] = str(isolated_path)
    doctor_env["SYMBROWSE_EXECUTABLE_PATH"] = sys.executable
    doctor_env["SYMBROWSE_ENCRYPTION_KEY"] = "0" * 64
    doctor_env["SYMBROWSE_SYMGUARD"] = "off"
    doctor_args = ["doctor", "--fix", "--json"]
    go_doctor = run_process(go, doctor_args, doctor_env)
    rust_doctor = run_process(rust, doctor_args, doctor_env)
    go_doctor_payload, go_doctor_payload_error = json_payload(go_doctor)
    rust_doctor_payload, rust_doctor_payload_error = json_payload(rust_doctor)
    comparisons.append({
        "case": "CLI-doctor-json-fix",
        "argv": doctor_args,
        "matched": all(go_doctor.get(key) == rust_doctor.get(key)
                       for key in ("returncode", "stdout", "stderr")),
        "criterion": "bounded doctor report, no provider lookup outside the disposable PATH, and Go-identical envelope/exit",
        "go": output_record(go_doctor),
        "rust": output_record(rust_doctor),
        "go_payload": go_doctor_payload,
        "go_payload_raw": go_doctor["stdout"].decode("utf-8", "replace"),
        "go_payload_error": go_doctor_payload_error,
        "rust_payload": rust_doctor_payload,
        "rust_payload_raw": rust_doctor["stdout"].decode("utf-8", "replace"),
        "rust_payload_error": rust_doctor_payload_error,
    })
    return comparisons


def compare_diff_snapshot(go: Path, rust: Path, env: dict[str, str],
                          output_args: list[str]) -> dict[str, Any]:
    baseline_path = Path(env["TMPDIR"]) / "diff-baseline.json"
    try:
        baseline_path.write_text(json.dumps({"tree": "before\nshared\n", "refs": {}}))
        row = compare_processes(
            go, rust,
            ["diff", "snapshot", "--baseline", str(baseline_path), "--session", "fixture", *output_args],
            b"", env, stub=True,
        )
        mode = "json" if "--json" in output_args else "text"
        row["case"] = f"CLI-001-diff-snapshot-baseline-{mode}"
        row["criterion"] = "baseline diff output and daemon frame match the Go command"
        return row
    except (OSError, RuntimeError) as error:
        mode = "json" if "--json" in output_args else "text"
        return {"case": f"CLI-001-diff-snapshot-baseline-{mode}", "matched": False,
                "error": str(error)}
    finally:
        try:
            baseline_path.unlink()
        except FileNotFoundError:
            pass


def compare_diff_url(go: Path, rust: Path, env: dict[str, str]) -> dict[str, Any]:
    argv = ["diff", "url", "https://fixture.invalid/before", "https://fixture.invalid/after",
            "--session", "fixture"]
    row = compare_processes(go, rust, argv, b"", env, stub=True)
    row["case"] = "CLI-001-diff-url"
    row["criterion"] = "both ordered read requests and the URL diff output match Go"
    return row


def compare_trace_export(go: Path, rust: Path, env: dict[str, str]) -> dict[str, Any]:
    argv = ["trace", "export", "--session", "fixture", "--out",
            str(Path(env["TMPDIR"]) / "trace-export.json"), "--json"]
    trace_path = Path(argv[5])

    def run(binary: Path, *, status_probe: bool) -> tuple[dict[str, Any], dict[str, Any] | None, str | None]:
        session = session_from_argv(argv)
        if os.name == "nt":
            with WindowsNamedPipeStub(
                session=session, status_probe=status_probe, request_count=2 if status_probe else 1
            ) as daemon:
                result = run_process(binary, argv, env)
            return result, daemon.frame, daemon.error
        with UnixDaemonStub(
            socket_path(env, session), status_probe=status_probe,
            request_count=2 if status_probe else 1,
        ) as daemon:
            result = run_process(binary, argv, env)
        return result, daemon.frame, daemon.error

    try:
        trace_path.unlink(missing_ok=True)
        go_result, go_frame, go_stub_error = run(go, status_probe=False)
        go_document = json.loads(trace_path.read_text()) if trace_path.is_file() else None
        trace_path.unlink(missing_ok=True)
        rust_result, rust_frame, rust_stub_error = run(rust, status_probe=True)
        rust_document = json.loads(trace_path.read_text()) if trace_path.is_file() else None
    except (OSError, RuntimeError, json.JSONDecodeError) as error:
        return {"case": "CLI-001-trace-export", "argv": argv, "matched": False,
                "error": str(error)}
    finally:
        try:
            trace_path.unlink()
        except FileNotFoundError:
            pass

    def stable_trace(value: Any) -> Any:
        if isinstance(value, dict):
            return {key: stable_trace(item) for key, item in value.items() if key != "created_at"}
        if isinstance(value, list):
            return [stable_trace(item) for item in value]
        return value

    def frame_payload(frame: dict[str, Any] | None) -> dict[str, Any] | None:
        if frame is None:
            return None
        return {key: frame.get(key) for key in ("cmd", "session", "args")}

    timestamp_valid = True
    for document in (go_document, rust_document):
        try:
            datetime.fromisoformat(document["created_at"].replace("Z", "+00:00"))
        except (TypeError, KeyError, ValueError, AttributeError):
            timestamp_valid = False
    result_match = all(
        go_result.get(key) == rust_result.get(key)
        for key in ("returncode", "stdout", "stderr")
    )
    matched = bool(
        result_match and go_result.get("returncode") == 0 and
        stable_trace(go_document) == stable_trace(rust_document) and
        timestamp_valid and frame_payload(go_frame) == frame_payload(rust_frame) and
        go_frame is not None and rust_frame is not None and
        go_frame.get("cmd") == "journal.show" and rust_frame.get("cmd") == "journal.show" and
        not go_stub_error and not rust_stub_error
    )
    return {
        "case": "CLI-001-trace-export", "argv": argv, "matched": matched,
        "criterion": "trace export file matches Go apart from its independently generated timestamp",
        "go": output_record(go_result), "rust": output_record(rust_result),
        "go_trace": go_document, "rust_trace": rust_document,
        "go_frame": frame_payload(go_frame), "rust_frame": frame_payload(rust_frame),
        "go_stub_error": go_stub_error, "rust_stub_error": rust_stub_error,
    }


def compare_trace_replay(go: Path, rust: Path, env: dict[str, str],
                         output_args: list[str] | None = None) -> dict[str, Any]:
    fixture = Path(env["TMPDIR"]) / "trace-replay.json"
    url = "https://fixture.invalid/replay"
    next_url = "https://fixture.invalid/next"
    document = {
        "schema_version": 1,
        "created_at": "2026-09-25T00:00:00Z",
        "session": "fixture",
        "steps": [
            {"command": "open", "url": url, "expected_url": url + "#section"},
            {"command": "open", "url": url + "/failed", "expected_url": url + "/failed"},
            {"command": "goto", "url": next_url, "expected_url": url},
            {"command": "click", "selector": "#button"},
            {"command": "dblclick", "selector": "#button"},
            {"command": "hover", "selector": "#button"},
            {"command": "focus", "selector": "#input"},
            {"command": "check", "selector": "#check"},
            {"command": "uncheck", "selector": "#check"},
            {"command": "scrollintoview", "selector": "#target"},
            {"command": "scroll", "selector": "body"},
            {"command": "fill", "selector": "#input", "value": "fixture"},
            {"command": "type", "selector": "#input", "value": "text"},
            {"command": "select", "selector": "#select", "value": "option"},
            {"command": "press", "key": "Enter"},
            {"command": "auth.login", "value": "fixture-entry"},
            {"command": "not-replayable"},
        ],
    }
    output_args = output_args or []
    argv = ["trace", "replay", str(fixture), "--session", "fixture", *output_args]
    try:
        fixture.write_text(json.dumps(document), encoding="utf-8")

        def run(binary: Path, *, rust_client: bool) -> tuple[dict[str, Any], list[dict[str, Any]], str | None]:
            rust_request_count = sum(
                step["command"] in TRACE_INTERACTIONS | {"open", "goto"}
                for step in document["steps"]
            )
            count = 2 * max(1, rust_request_count) if rust_client else 1
            if os.name == "nt":
                with WindowsNamedPipeStub(
                    session="fixture", status_probe=rust_client, request_count=count
                ) as daemon:
                    result = run_process(binary, argv, env)
            else:
                with UnixDaemonStub(
                    socket_path(env, "fixture"), status_probe=rust_client, request_count=count
                ) as daemon:
                    result = run_process(binary, argv, env)
            return result, daemon.frames, daemon.error

        go_result, go_frames, go_stub_error = run(go, rust_client=False)
        rust_result, rust_frames, rust_stub_error = run(rust, rust_client=True)
    except (OSError, RuntimeError) as error:
        return {"case": "CLI-001-trace-replay-open", "argv": argv,
                "matched": False, "error": str(error)}
    finally:
        try:
            fixture.unlink()
        except FileNotFoundError:
            pass

    go_requests = [frame for frame in go_frames if frame.get("cmd") != "daemon.status"]
    rust_requests = [frame for frame in rust_frames if frame.get("cmd") != "daemon.status"]
    same_output = all(
        go_result.get(key) == rust_result.get(key)
        for key in ("returncode", "stdout", "stderr")
    )
    expected_rust_requests = []
    for step in document["steps"]:
        command = step["command"]
        if command in ("auth.login", "not-replayable"):
            continue
        if command in ("open", "goto"):
            args = {"url": step["url"]}
        elif command == "press":
            args = {"action": command, "selector": "body", "key": step["key"]}
        elif command == "scroll":
            args = {"action": command, "selector": step["selector"], "amount": 1}
        elif command in ("fill", "type", "select"):
            args = {"action": command, "selector": step["selector"], "value": step["value"]}
        else:
            args = {"action": command, "selector": step["selector"]}
        expected_rust_requests.append({"cmd": command, "session": "fixture", "args": args})
    protocol_shape = (
        len(go_requests) == 1
        and go_requests[0].get("cmd") == "trace.replay"
        and (go_requests[0].get("args") or {}).get("steps") == document["steps"]
        and [{key: frame.get(key) for key in ("cmd", "session", "args")} for frame in rust_requests]
        == expected_rust_requests
    )
    return {
        "case": "CLI-001-trace-replay-actions" + ("-" + output_args[0].lstrip("-").replace("=", "-") if output_args else "-text"),
        "argv": argv,
        "matched": bool(
            same_output and go_result.get("returncode") == 0 and protocol_shape
            and not go_stub_error and not rust_stub_error
        ),
        "criterion": "all replayable action frames and credential/unknown outcomes match Go semantics",
        "go": output_record(go_result),
        "rust": output_record(rust_result),
        "go_request": go_requests[0] if go_requests else None,
        "rust_request": rust_requests[0] if rust_requests else None,
        "go_stub_error": go_stub_error,
        "rust_stub_error": rust_stub_error,
    }


def compare_trace_replay_empty(go: Path, rust: Path, env: dict[str, str],
                               output_args: list[str]) -> dict[str, Any]:
    fixture = Path(env["TMPDIR"]) / "trace-replay-empty.json"
    argv = ["trace", "replay", str(fixture), "--session", "fixture", *output_args]
    document = {"schema_version": 1, "created_at": "2026-09-25T00:00:00Z",
                "session": "fixture", "steps": []}
    try:
        fixture.write_text(json.dumps(document), encoding="utf-8")

        def run(binary: Path, *, rust_client: bool) -> tuple[dict[str, Any], list[dict[str, Any]], str | None]:
            if os.name == "nt":
                with WindowsNamedPipeStub(session="fixture", status_probe=rust_client,
                                          request_count=2 if rust_client else 1) as daemon:
                    result = run_process(binary, argv, env)
            else:
                with UnixDaemonStub(socket_path(env, "fixture"), status_probe=rust_client,
                                    request_count=2 if rust_client else 1) as daemon:
                    result = run_process(binary, argv, env)
            return result, daemon.frames, daemon.error

        go_result, go_frames, go_error = run(go, rust_client=False)
        rust_result, rust_frames, rust_error = run(rust, rust_client=True)
    except (OSError, RuntimeError) as error:
        return {"case": "CLI-001-trace-replay-empty", "argv": argv,
                "matched": False, "error": str(error)}
    finally:
        try:
            fixture.unlink()
        except FileNotFoundError:
            pass

    go_requests = [frame for frame in go_frames if frame.get("cmd") != "daemon.status"]
    rust_requests = [frame for frame in rust_frames if frame.get("cmd") != "daemon.status"]
    expected = {"cmd": "trace.replay", "session": "fixture", "args": {"steps": []}}
    output_match = all(go_result.get(key) == rust_result.get(key)
                       for key in ("returncode", "stdout", "stderr"))
    return {
        "case": "CLI-001-trace-replay-empty" + ("-" + output_args[0].lstrip("-").replace("=", "-") if output_args else "-text"),
        "argv": argv,
        "matched": bool(output_match and trace_frames_match(go_requests, rust_requests, expected)
                        and not go_error and not rust_error),
        "criterion": "empty trace reaches the daemon and preserves its failure envelope",
        "go": output_record(go_result), "rust": output_record(rust_result),
        "go_request": go_requests[0] if go_requests else None,
        "rust_request": rust_requests[0] if rust_requests else None,
        "go_stub_error": go_error, "rust_stub_error": rust_error,
    }


def trace_frames_match(go_frames: list[dict[str, Any]], rust_frames: list[dict[str, Any]],
                       expected: dict[str, Any]) -> bool:
    """Compare full request metadata while checking the expected trace payload."""
    if len(go_frames) != 1 or len(rust_frames) != 1 or go_frames != rust_frames:
        return False
    frame = go_frames[0]
    return (all(frame.get(key) == value for key, value in expected.items())
            and frame.get("request_id") == "1"
            and frame.get("retrieval_surface") == "cli")


def compare_trace_schema_error(go: Path, rust: Path, env: dict[str, str],
                               output_args: list[str]) -> dict[str, Any]:
    fixture = Path(env["TMPDIR"]) / "trace-replay-schema.json"
    argv = ["trace", "replay", str(fixture), "--session", "fixture", *output_args]
    try:
        fixture.write_text(json.dumps({"schema_version": 2, "created_at": "",
                                       "session": "fixture", "steps": []}), encoding="utf-8")
        go_result = run_process(go, argv, env)
        rust_result = run_process(rust, argv, env)
    except OSError as error:
        return {"case": "CLI-001-trace-replay-schema", "argv": argv,
                "matched": False, "error": str(error)}
    finally:
        try:
            fixture.unlink()
        except FileNotFoundError:
            pass
    matched = all(go_result.get(key) == rust_result.get(key)
                  for key in ("returncode", "stdout", "stderr"))
    return {
        "case": "CLI-001-trace-replay-schema" + ("-" + output_args[0].lstrip("-").replace("=", "-") if output_args else "-text"),
        "argv": argv, "matched": bool(matched),
        "criterion": "unsupported schema uses the same internal error in text, JSON, and YAML",
        "go": output_record(go_result), "rust": output_record(rust_result),
    }


def without_batch_durations(value: Any) -> Any:
    """Remove execution timings, which are intentionally nondeterministic."""
    if isinstance(value, dict):
        return {key: without_batch_durations(item) for key, item in value.items()
                if key != "duration_ms"}
    if isinstance(value, list):
        return [without_batch_durations(item) for item in value]
    return value


def compare_json_semantic(go: Path, rust: Path, argv: list[str], env: dict[str, str],
                          *, case: str) -> dict[str, Any]:
    go_result = run_process(go, argv, env)
    rust_result = run_process(rust, argv, env)
    go_value: Any = None
    rust_value: Any = None
    parse_error: str | None = None
    try:
        go_value = json.loads(go_result["stdout"])
        rust_value = json.loads(rust_result["stdout"])
    except (TypeError, json.JSONDecodeError) as error:
        parse_error = str(error)
    semantic_match = parse_error is None and without_batch_durations(go_value) == without_batch_durations(rust_value)
    matched = (semantic_match and go_result.get("returncode") == rust_result.get("returncode")
               and go_result.get("stderr") == rust_result.get("stderr")
               and not go_result.get("error") and not rust_result.get("error"))
    return {"case": case, "argv": argv, "matched": matched,
            "criterion": "JSON semantics match after removing Go/Rust per-item timing",
            "parse_error": parse_error, "go": output_record(go_result),
            "rust": output_record(rust_result),
            "go_data": without_batch_durations(go_value),
            "rust_data": without_batch_durations(rust_value)}


def run_batch_cases(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    """Exercise the bounded batch behavior shared by the current Go and Rust CLIs."""
    cases = [
        ("CLI-006", ["batch", "version --json", "version extra", "version --json"]),
        ("CLI-006", ["batch", "--bail", "version extra", "version --json"]),
        ("CLI-006", ["batch", "--dry-run", "open https://fixture.invalid", "version --json"]),
    ]
    comparisons = [compare_json_semantic(go, rust, argv, env, case=contract)
                   for contract, argv in cases]
    for row in comparisons:
        if row["case"] == "CLI-006" and row["matched"]:
            value = row["go_data"]
            report = value if isinstance(value, dict) else {}
            results = report.get("results", [])
            if row["argv"][1] == "version --json":
                row["matched"] = (
                    isinstance(results, list) and len(results) == 3
                    and all(isinstance(item, dict) for item in results)
                    and [item.get("command") for item in results] ==
                    ["version --json", "version extra", "version --json"]
                    and isinstance(results[0].get("data"), dict)
                    and results[0]["data"].get("tool") == "symbrowse"
                    and results[1].get("success") is False
                    and results[2].get("success") is True
                )
                row["criterion"] = "ordered mixed results and nested JSON data are preserved"
            elif "--bail" in row["argv"]:
                row["matched"] = (isinstance(results, list) and len(results) == 1
                                   and isinstance(results[0], dict)
                                   and results[0].get("success") is False
                                   and report.get("bailed") is True)
                row["criterion"] = "--bail stops after the first failed command"
            else:
                plan = report.get("plan", [])
                row["matched"] = (isinstance(plan, list) and len(plan) == 2
                                   and all(isinstance(item, dict) for item in plan)
                                   and not report.get("results")
                                   and [item.get("command") for item in plan] ==
                                   ["open https://fixture.invalid", "version --json"]
                                   and [item.get("risk_class") for item in plan] == ["navigate", "unknown"])
                row["criterion"] = "dry-run returns ordered risk plan and no command results or side effects"
    yaml_cases = [
        ("OUT-003", ["batch", "--output=yaml", "--dry-run",
                     "open https://fixture.invalid", "version --json"]),
        ("OUT-003", ["config", "show"]),
        ("OUT-003", ["config", "show", "--output", "yaml"]),
        ("OUT-003", ["profiles"]),
        ("OUT-003", ["profiles", "--output", "yaml"]),
        ("OUT-003", ["flow", "list"]),
        ("OUT-003", ["flow", "list", "--output", "yaml"]),
        ("OUT-003", ["version"]),
        ("OUT-003", ["version", "--output", "yaml"]),
    ]
    for contract, argv in yaml_cases:
        go_result = run_process(go, argv, env)
        rust_result = run_process(rust, argv, env)
        matched = all(go_result.get(key) == rust_result.get(key)
                      for key in ("returncode", "stdout", "stderr"))
        comparisons.append({"case": contract, "argv": argv, "matched": matched,
                            "criterion": "byte-identical Go/Rust human or YAML output",
                            "go": output_record(go_result), "rust": output_record(rust_result)})
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
    short_path_links: list[Path] = []
    if sys.platform == "darwin":
        short_home_link = Path("/tmp") / f"symbrowse-cli-home-{os.getpid()}"
        try:
            short_home_link.symlink_to(env["HOME"], target_is_directory=True)
        except FileExistsError:
            parser.error(f"temporary macOS HOME link already exists: {short_home_link}")
        env["HOME"] = str(short_home_link)
        short_path_links.append(short_home_link)
    elif sys.platform.startswith("linux"):
        short_runtime_link = Path("/tmp") / f"symbrowse-cli-runtime-{os.getpid()}"
        try:
            short_runtime_link.symlink_to(env["XDG_RUNTIME_DIR"], target_is_directory=True)
        except FileExistsError:
            parser.error(f"temporary Linux runtime link already exists: {short_runtime_link}")
        env["XDG_RUNTIME_DIR"] = str(short_runtime_link)
        short_path_links.append(short_runtime_link)
    elif os.name == "nt":
        windows_runtime = Path(tempfile.gettempdir()) / f"symbrowse-cli-runtime-{os.getpid()}"
        try:
            windows_runtime.mkdir(mode=0o700)
        except FileExistsError:
            parser.error(f"temporary Windows runtime directory already exists: {windows_runtime}")
        env["XDG_RUNTIME_DIR"] = str(windows_runtime)
    if os.name in {"posix", "nt"}:
        max_socket_path_bytes = 103 if sys.platform == "darwin" or os.name == "nt" else 107
        encoded_path = os.fsencode(socket_path(env))
        if len(encoded_path) > max_socket_path_bytes:
            for link in short_path_links:
                link.unlink()
            parser.error(
                f"Unix daemon test socket path is {len(encoded_path)} bytes; "
                f"this platform permits at most {max_socket_path_bytes}. "
                "Use --temp-root on a shorter path."
            )
    rows = [] if args.skip_help_tree else help_tree(args.go.resolve(), args.rust.resolve(), env)
    rows.extend(implemented_help(args.go.resolve(), args.rust.resolve(), env))
    rows.extend(completion_oracle_cases(args.go.resolve(), args.rust.resolve(), env))
    rows.extend(completion_candidate_cases(args.go.resolve(), args.rust.resolve(), env))
    rows.extend(run_fixed_cases(args.go.resolve(), args.rust.resolve(), env))
    rows.extend(run_batch_cases(args.go.resolve(), args.rust.resolve(), env))
    rows.extend(run_watch_cases(args.go.resolve(), args.rust.resolve(), env))
    report = {
        "schema_version": 1,
        "source_head": subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip(),
        "go_binary_sha256": sha256(args.go.read_bytes()),
        "rust_binary_sha256": sha256(args.rust.read_bytes()),
        "platform": sys.platform,
        "isolated_env_root": str(common),
        "short_socket_path_links": [str(link) for link in short_path_links],
        "rows": rows,
        "summary": {"cases": len(rows), "matched": sum(row.get("matched") is True for row in rows),
                    "mismatched": sum(row.get("matched") is False for row in rows),
                    "unsupported": sum(row.get("case") == "unsupported" for row in rows),
                    "harness_errors": sum(row.get("case") == "harness_error" for row in rows)},
        "parity_established": False,
    }
    args.report.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    for link in short_path_links:
        link.unlink()
    print(json.dumps(report["summary"], sort_keys=True))
    print(f"report: {args.report}")
    return 0 if all(report["summary"][key] == 0 for key in ("mismatched", "unsupported", "harness_errors")) else 1


if __name__ == "__main__":
    raise SystemExit(main())
