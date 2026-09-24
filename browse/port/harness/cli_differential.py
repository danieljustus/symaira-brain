#!/usr/bin/env python3
"""Byte-compare selected Go and Rust Browse CLI contracts in isolation."""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import threading
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any
from ctypes import wintypes


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


def implemented_help(go: Path, rust: Path, env: dict[str, str]) -> list[dict[str, Any]]:
    expected = {
        "a11y", "back", "batch", "cache", "check", "click", "config", "daemon", "dblclick", "dialog", "eval", "fetch", "fill", "find",
        "flow", "focus", "forward", "frame", "get", "goto", "hover", "is", "mcp", "open", "press", "profiles",
        "read", "reload", "screenshot", "scrollintoview", "select", "session", "set", "snapshot", "state", "storage", "tab", "tools", "type", "uncheck", "version", "wait", "workflow",
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
        ["a11y"], ["batch"], ["config"], ["config", "show"], ["dialog"], ["dialog", "accept"], ["screenshot"],
        ["dialog", "auto"], ["dialog", "dismiss"], ["dialog", "status"],
        ["tab"], ["tab", "list"], ["tab", "new"], ["tab", "switch"], ["tab", "close"],
        ["tab", "window"], ["tab", "window", "window"],
        ["session"], ["session", "id"], ["session", "list"], ["session", "info"],
        ["cache", "get"],
        ["check"], ["dblclick"], ["focus"], ["hover"], ["select"], ["uncheck"], ["scrollintoview"],
        ["frame", "tree"], ["set", "offline"], ["eval"], ["flow", "list"],
        ["flow", "run"], ["flow", "validate"], ["mcp"], ["profiles"], ["state"],
        ["state", "clean"], ["state", "clear"], ["state", "key"], ["state", "key", "init"],
        ["state", "list"], ["state", "load"], ["state", "save"], ["state", "show"],
        ["storage"], ["storage", "clear"], ["storage", "get"], ["storage", "set"], ["version"],
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
                    if frame.get("cmd") == "daemon.status":
                        data = {"session": frame.get("session", "default")}
                    elif frame.get("cmd") == "storage.list":
                        args = frame.get("args") or {}
                        data = {"origin": "https://fixture.invalid", "kind": args.get("kind", ""),
                                "items": {"alpha": "one", "beta": "two"}}
                    elif frame.get("cmd") == "storage.set":
                        data = {"set": (frame.get("args") or {}).get("key", "")}
                    elif frame.get("cmd") == "storage.clear":
                        data = {"cleared": (frame.get("args") or {}).get("kind", "")}
                    elif frame.get("cmd") == "session.list":
                        data = {"schema_version": 1, "sessions": []}
                    elif frame.get("cmd") == "session.info":
                        data = {"name": frame.get("session", "default"), "active_tabs": 0}
                    elif frame.get("cmd") == "a11y":
                        data = {"nodes": []}
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


class WindowsNamedPipeStub:
    """Bounded byte-stream stub for the production Windows daemon pipe."""

    PIPE = rf"\\.\pipe\symbrowse-{SESSION}"
    PIPE_ACCESS_DUPLEX = 0x00000003
    PIPE_TYPE_BYTE = 0x00000000
    PIPE_READMODE_BYTE = 0x00000000
    PIPE_WAIT = 0x00000000
    ERROR_PIPE_CONNECTED = 535
    INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

    def __init__(self, *, status_probe: bool, request_count: int | None = None):
        self.frame: dict[str, Any] | None = None
        self.frames: list[dict[str, Any]] = []
        self.error: str | None = None
        self.status_probe = status_probe
        self.request_count = request_count
        self.thread: threading.Thread | None = None
        self.ready = threading.Event()
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
        self.kernel.DisconnectNamedPipe.argtypes = [wintypes.HANDLE]
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]

    def __enter__(self) -> "WindowsNamedPipeStub":
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()
        if not self.ready.wait(timeout=5):
            raise RuntimeError(self.error or "Windows daemon test pipe was not ready")
        return self

    def _win_error(self, operation: str) -> OSError:
        return ctypes.WinError(ctypes.get_last_error(), operation)

    def _serve(self) -> None:
        try:
            request_count = self.request_count if self.request_count is not None else (2 if self.status_probe else 1)
            for request_index in range(request_count):
                handle = self.kernel.CreateNamedPipeW(
                    self.PIPE, self.PIPE_ACCESS_DUPLEX,
                    self.PIPE_TYPE_BYTE | self.PIPE_READMODE_BYTE | self.PIPE_WAIT,
                    1, MAX_CAPTURE_BYTES, MAX_CAPTURE_BYTES, 5000, None,
                )
                if handle == self.INVALID_HANDLE_VALUE:
                    raise self._win_error("CreateNamedPipeW")
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
                    frame = json.loads(bytes(raw).split(b"\n", 1)[0])
                    self.frames.append(frame)
                    if request_index == 0:
                        self.frame = frame
                    if frame.get("cmd") == "daemon.status":
                        data = {"session": frame.get("session", SESSION)}
                    elif frame.get("cmd") == "storage.list":
                        args = frame.get("args") or {}
                        data = {"origin": "https://fixture.invalid", "kind": args.get("kind", ""),
                                "items": {"alpha": "one", "beta": "two"}}
                    elif frame.get("cmd") == "storage.set":
                        data = {"set": (frame.get("args") or {}).get("key", "")}
                    elif frame.get("cmd") == "storage.clear":
                        data = {"cleared": (frame.get("args") or {}).get("kind", "")}
                    elif frame.get("cmd") == "session.list":
                        data = {"schema_version": 1, "sessions": []}
                    elif frame.get("cmd") == "session.info":
                        data = {"name": frame.get("session", SESSION), "active_tabs": 0}
                    elif frame.get("cmd") == "a11y":
                        data = {"nodes": []}
                    else:
                        args = frame.get("args") or {}
                        data = {"url": args.get("url", ""), "value": 2,
                                "expression": args.get("expression", "")}
                    response = json.dumps(
                        {"success": True, "data": data, "warnings": []},
                        separators=(",", ":"),
                    ).encode() + b"\n"
                    written = wintypes.DWORD()
                    buffer = ctypes.create_string_buffer(response, len(response))
                    if not self.kernel.WriteFile(handle, buffer, len(response), ctypes.byref(written), None):
                        raise self._win_error("WriteFile")
                    if written.value != len(response):
                        raise RuntimeError("short write to Windows daemon pipe")
                finally:
                    self.kernel.DisconnectNamedPipe(handle)
                    self.kernel.CloseHandle(handle)
        except Exception as error:  # retained in the bounded report
            self.error = str(error)
            self.ready.set()

    def __exit__(self, *_: object) -> None:
        if self.thread is not None:
            self.thread.join(timeout=16)


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
        try:
            has_a11y_url = (argv[:1] == ["a11y"] and any(
                value.startswith(("http://", "https://")) for value in argv[1:]))
            go_count = 2 if has_a11y_url else 1
            rust_count = 4 if has_a11y_url else 2
            if os.name == "nt":
                with WindowsNamedPipeStub(status_probe=False, request_count=go_count) as go_stub:
                    go_result = run_process(go, argv, env, stdin)
                go_frame = go_stub.frame
                go_frames = go_stub.frames
                go_error = go_stub.error
                with WindowsNamedPipeStub(status_probe=True, request_count=rust_count) as rust_stub:
                    rust_result = run_process(rust, argv, env, stdin)
                rust_frame = rust_stub.frame
                rust_frames = rust_stub.frames
                rust_error = rust_stub.error
            else:
                go_path = socket_path(env)
                with UnixDaemonStub(go_path, status_probe=False, request_count=go_count) as go_stub:
                    go_result = run_process(go, argv, env, stdin)
                go_frame = go_stub.frame
                go_frames = go_stub.frames
                go_error = go_stub.error
                rust_path = socket_path(env)
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
        if argv[:1] == ["a11y"]:
            def actual_frames(frames: list[dict[str, Any]]) -> list[dict[str, Any]]:
                return [payload_raw(frame) for frame in frames if frame.get("cmd") != "daemon.status"]

            go_actual = actual_frames(go_frames)
            rust_actual = actual_frames(rust_frames)
            row["go_actual_frames"] = go_actual
            row["rust_actual_frames"] = rust_actual
            row["daemon_actual_frames_match"] = go_actual == rust_actual
            row["matched"] = bool(row["matched"] and row["daemon_actual_frames_match"])
    return row


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
        ("CLI-003", ["check"], b"", False),
        ("CLI-003", ["check", "#agree", "extra"], b"", False),
        ("CLI-003", ["select", "#country", "DE", "extra"], b"", False),
        ("CLI-003", ["scrollintoview", "#target", "extra"], b"", False),
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
    return comparisons


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
                                   ["open https://fixture.invalid", "version --json"])
                row["criterion"] = "dry-run returns a plan and does not run command items"
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
    rows.extend(run_fixed_cases(args.go.resolve(), args.rust.resolve(), env))
    rows.extend(run_batch_cases(args.go.resolve(), args.rust.resolve(), env))
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
