#!/usr/bin/env python3
"""Run small, hermetic Go/Rust release-value probes.

This is deliberately a measurement runner, not a cutover switch.  It covers
CLI, MCP, daemon IPC and a local static-fetch probe when the binary implements
the corresponding surface.  Unsupported or failed workloads remain visible in
the JSON report and never become a passing value gate.
"""
from __future__ import annotations

import argparse
from collections.abc import Collection
import ctypes
import hashlib
import json
import os
import platform
import secrets
import signal
import sys
try:
    import resource
except ImportError:  # pragma: no cover - resource is not available on native Windows
    resource = None  # type: ignore[assignment]
import socket
import statistics
import subprocess
import tempfile
import threading
import time
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Sequence

scripts_directory = Path(__file__).resolve().parents[3] / "scripts"
if str(scripts_directory) not in sys.path:
    sys.path.insert(0, str(scripts_directory))
from external_env import ensure_external_environment

MAX_OUTPUT = 1 << 20
WORKLOADS = ("cli", "mcp", "daemon", "fetch")
EXTERNAL_RUNTIME_ENV = "SYMAIRA_EXTERNAL_RUNTIME_ROOT"
EXTERNAL_RUNTIME_ROOT = Path("/Volumes/1TB_NVMe_SN850X")
_TEMP_HOME_ALIASES: list[Any] = []


def temporary_parent() -> str | None:
    if platform.system() != "Darwin" or os.environ.get("CI"):
        return None
    try:
        mounted_root = EXTERNAL_RUNTIME_ROOT.resolve(strict=True)
    except OSError as error:
        raise RuntimeError(
            f"required NVMe runtime volume is unavailable: {EXTERNAL_RUNTIME_ROOT}: {error}"
        ) from error
    if not mounted_root.is_dir() or not mounted_root.is_mount():
        raise RuntimeError(f"required NVMe runtime volume is not mounted: {mounted_root}")
    value = os.environ.get(EXTERNAL_RUNTIME_ENV, "").strip()
    if not value:
        raise RuntimeError(
            f"{EXTERNAL_RUNTIME_ENV} must name a writable directory under {mounted_root} on macOS"
        )
    parent = Path(value).expanduser()
    try:
        parent = parent.resolve(strict=True)
    except OSError as error:
        raise RuntimeError(f"{EXTERNAL_RUNTIME_ENV} is invalid: {value!r}: {error}") from error
    if not parent.is_relative_to(mounted_root):
        raise RuntimeError(
            f"{EXTERNAL_RUNTIME_ENV} must be under mounted NVMe volume {mounted_root}: {parent}"
        )
    if not parent.is_dir() or not os.access(parent, os.W_OK | os.X_OK):
        raise RuntimeError(
            f"{EXTERNAL_RUNTIME_ENV} must name a writable directory: {parent}"
        )
    return str(parent)


def external_output(path: Path) -> Path:
    if platform.system() != "Darwin" or os.environ.get("CI"):
        return path
    temporary_parent()
    resolved = path.expanduser().resolve(strict=False)
    mounted_root = EXTERNAL_RUNTIME_ROOT.resolve(strict=True)
    if not resolved.is_relative_to(mounted_root):
        raise RuntimeError(f"--output must be under mounted NVMe volume {mounted_root}: {resolved}")
    return resolved


@dataclass(frozen=True)
class Probe:
    name: str
    argv: tuple[str, ...]
    stdin: str = ""


class FixtureHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802 - stdlib callback name
        body = b"<html><head><title>RUST-016</title></head><body><main><p>fixture</p></main></body></html>\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        return


def base_env(root: Path) -> dict[str, str]:
    isolated_home = root / "home"
    isolated_home.mkdir(mode=0o700, exist_ok=True)
    home = isolated_home
    if platform.system() == "Darwin":
        # AF_UNIX paths have a small SUN_LEN limit. A lexical /tmp symlink
        # keeps macOS daemon probes runnable while its target and all contents
        # remain under the already-validated external temporary root.
        alias_dir = tempfile.TemporaryDirectory(prefix="sb-bench-", dir="/tmp")
        _TEMP_HOME_ALIASES.append(alias_dir)
        home = Path(alias_dir.name) / "home"
        home.symlink_to(isolated_home, target_is_directory=True)
    runtime = root / "runtime"
    cache = root / "cache"
    for path in (home, runtime, cache):
        path.mkdir(mode=0o700, exist_ok=True)
    env = {
        "HOME": str(home),
        "USERPROFILE": str(home),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "XDG_DATA_HOME": str(home / ".local" / "share"),
        "XDG_CACHE_HOME": str(cache),
        "XDG_STATE_HOME": str(home / ".local" / "state"),
        "XDG_RUNTIME_DIR": str(runtime),
        "TMPDIR": str(root / "tmp"),
        "TMP": str(root / "tmp"),
        "TEMP": str(root / "tmp"),
        "LANG": "C",
        "LC_ALL": "C",
        "TZ": "UTC",
        "TERM": "dumb",
        "NO_COLOR": "1",
        "SYMBROWSE_CHECK_UPDATES": "0",
        "SYMBROWSE_SYMGUARD": "off",
        "SYMBROWSE_ALLOW_PRIVATE": "true",
    }
    if os.name == "nt":
        env["APPDATA"] = str(home / "AppData" / "Roaming")
        env["LOCALAPPDATA"] = str(home / "AppData" / "Local")
    (root / "tmp").mkdir(mode=0o700, exist_ok=True)
    for key in ("PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT"):
        if key in os.environ:
            env[key] = os.environ[key]
    return env



def implementation_env(root: Path, implementation: str) -> dict[str, str]:
    """Apply only the selection variable understood by each CLI."""
    env = base_env(root)
    if implementation == "go":
        env["SYMBROWSE_ENGINE"] = "static"
    elif implementation == "rust":
        # Rust validates the loaded browser config before daemon flags apply.
        # Leave that config in its valid default; --mode static is the explicit
        # daemon selection and clears the inherited browser engine in the CLI.
        env["SYMBROWSE_MODE"] = "browser"
    else:
        raise ValueError(f"unknown implementation: {implementation}")
    return env


def terminate_process_tree(process: subprocess.Popen[bytes]) -> None:
    if getattr(process, "_bench_tree_terminated", False):
        return
    # Every owned probe starts a new session. Clean that group even when the
    # leader has exited; descendants may still retain files or sockets.
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            pass
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    elif process.poll() is None:
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], check=False,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3)
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        return
    # Avoid signaling a recycled group ID when a caller's finally repeats
    # cleanup after startup_failure already terminated the owned group.
    setattr(process, "_bench_tree_terminated", True)


def read_startup_diagnostic(path: Path, limit: int = 1 << 20) -> str:
    try:
        with path.open("rb") as stream:
            data = stream.read(limit + 1)
    except OSError as error:
        return f"<unable to read startup diagnostic: {error}>"
    if len(data) > limit:
        return "<startup diagnostic exceeded output limit>"
    detail = data.decode("utf-8", errors="replace").strip()
    return detail or "startup process exited without diagnostics"


def child_peak_rss_bytes() -> int | None:
    if resource is None:
        return None
    value = int(resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss)
    # macOS reports bytes; Linux and the BSDs report KiB.
    return value if platform.system() == "Darwin" else value * 1024


def run_once(binary: Path, probe: Probe, env: dict[str, str], cwd: Path) -> dict[str, object]:
    started = time.perf_counter_ns()
    rss_before = child_peak_rss_bytes()
    try:
        result = subprocess.run(
            [str(binary), *probe.argv],
            cwd=cwd,
            env=env,
            input=probe.stdin,
            text=True,
            capture_output=True,
            timeout=15,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {
            "status": "error",
            "reason": "timeout",
            "duration_ns": time.perf_counter_ns() - started,
            "peak_rss_bytes": child_peak_rss_bytes(),
        }
    duration = time.perf_counter_ns() - started
    rss_after = child_peak_rss_bytes()
    peak_rss = None if rss_before is None or rss_after is None else max(0, rss_after - rss_before)
    stdout = result.stdout[:MAX_OUTPUT]
    stderr = result.stderr[:MAX_OUTPUT]
    if len(result.stdout) > MAX_OUTPUT or len(result.stderr) > MAX_OUTPUT:
        return {"status": "error", "reason": "output limit exceeded", "duration_ns": duration, "peak_rss_bytes": peak_rss}
    lowered = (stdout + stderr).lower()
    if b"password=" in lowered.encode() or b"token=" in lowered.encode():
        return {"status": "error", "reason": "secret-like output", "duration_ns": duration, "peak_rss_bytes": peak_rss}
    if result.returncode == 0 and not stdout and not stderr:
        return {
            "status": "unsupported",
            "reason": "binary accepted command without observable output",
            "duration_ns": duration,
            "peak_rss_bytes": peak_rss,
        }
    if result.returncode != 0:
        return {
            "status": "unsupported" if "unknown command" in stderr.lower() or "not implemented" in stderr.lower() else "error",
            "reason": f"exit {result.returncode}",
            "duration_ns": duration,
            "peak_rss_bytes": peak_rss,
            "stdout_sha256": __import__("hashlib").sha256(stdout.encode()).hexdigest(),
            "stderr_sha256": __import__("hashlib").sha256(stderr.encode()).hexdigest(),
        }
    return {
        "status": "pass",
        "duration_ns": duration,
        "peak_rss_bytes": peak_rss,
        "stdout_bytes": len(stdout),
        "stderr_bytes": len(stderr),
    }


def summarize(samples: list[dict[str, object]]) -> dict[str, object]:
    passed = [
        int(duration)
        for item in samples
        if item.get("status") == "pass"
        and isinstance((duration := item.get("duration_ns")), (int, float))
    ]
    statuses = [str(item.get("status")) for item in samples]
    if len(passed) != len(samples):
        return {"status": statuses[0] if statuses else "error", "samples": samples}
    ordered = sorted(passed)
    p95 = ordered[max(0, (len(ordered) * 95 + 99) // 100 - 1)]
    peak_rss = [
        int(value)
        for item in samples
        if item.get("status") == "pass"
        and isinstance((value := item.get("peak_rss_bytes")), (int, float))
        and value > 0
    ]
    summary = {
        "status": "pass",
        "samples": len(passed),
        "raw_samples": [
            {
                "duration_ns": item.get("duration_ns"),
                "peak_rss_bytes": item.get("peak_rss_bytes"),
            }
            for item in samples
        ],
        "median_duration_ns": int(statistics.median(passed)),
        "p95_duration_ns": p95,
        "p95_calculation": "nearest-rank: sorted_samples[ceil(0.95*n)-1]",
        "statuses": statuses,
    }
    if peak_rss:
        summary["median_peak_rss_bytes"] = int(statistics.median(peak_rss))
    return summary


def daemon_command(binary: Path, session: str, *, static_mode: bool) -> list[str]:
    if static_mode:
        return [str(binary), "daemon", "--session", session, "--mode", "static"]
    return [str(binary), "daemon", "--session", session, "--engine", "static"]


def startup_failure(process: subprocess.Popen[bytes], prefix: str, stderr_path: Path) -> dict[str, object]:
    """Terminate descendants, then read a bounded file (never a pipe join)."""
    terminate_process_tree(process)
    detail = read_startup_diagnostic(stderr_path)
    if "password=" in detail.lower() or "token=" in detail.lower():
        detail = "secret-like startup diagnostic suppressed"
    return {"status": "error", "reason": f"{prefix}: {detail[:256]}"}


def launch_daemon(command: list[str], root: Path, env: dict[str, str]) -> tuple[subprocess.Popen[bytes], Path]:
    fd, name = tempfile.mkstemp(prefix="symbrowse-startup-", suffix=".log", dir=root / "tmp")
    stderr_path = Path(name)
    try:
        process_options: dict[str, object] = (
            {"start_new_session": True}
            if os.name == "posix"
            else {"creationflags": getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)}
        )
        with os.fdopen(fd, "wb") as stderr:
            process = subprocess.Popen(command, cwd=root, env=env, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.DEVNULL, stderr=stderr, **process_options)
    except BaseException:
        stderr_path.unlink(missing_ok=True)
        raise
    return process, stderr_path


def daemon_endpoint(session: str, env: dict[str, str]) -> str | Path:
    """Return the isolated per-probe daemon endpoint for this host."""
    if os.name == "nt":
        # Matches symbrowse-daemon's default_socket_path on Windows. Session
        # names are randomized per probe and never derived from user data.
        return rf"\\.\pipe\symbrowse-{session}"
    if platform.system() == "Darwin":
        return (
            Path(env["HOME"])
            / "Library"
            / "Caches"
            / "symbrowse"
            / "run"
            / f"{session}.sock"
        )
    return Path(env["XDG_RUNTIME_DIR"]) / "symbrowse" / f"{session}.sock"


def probe_session(name: str) -> str:
    """Avoid attaching to a user's or another benchmark's named-pipe daemon."""
    return f"{name}-{secrets.token_hex(8)}"


def _windows_pipe_api() -> tuple[Any, Any, Any, Any]:
    """Bind only the Win32 calls needed to exchange bounded JSON-line frames."""
    if os.name != "nt":
        raise OSError("Windows named pipes are available only on Windows")
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    wait_named_pipe = kernel32.WaitNamedPipeW
    wait_named_pipe.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32]
    wait_named_pipe.restype = ctypes.c_int
    create_file = kernel32.CreateFileW
    create_file.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32,
                            ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
    create_file.restype = ctypes.c_void_p
    set_pipe_state = kernel32.SetNamedPipeHandleState
    set_pipe_state.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32),
                               ctypes.c_void_p, ctypes.c_void_p]
    set_pipe_state.restype = ctypes.c_int
    read_file = kernel32.ReadFile
    read_file.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32,
                          ctypes.POINTER(ctypes.c_uint32), ctypes.c_void_p]
    read_file.restype = ctypes.c_int
    write_file = kernel32.WriteFile
    write_file.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32,
                           ctypes.POINTER(ctypes.c_uint32), ctypes.c_void_p]
    write_file.restype = ctypes.c_int
    close_handle = kernel32.CloseHandle
    close_handle.argtypes = [ctypes.c_void_p]
    close_handle.restype = ctypes.c_int
    return wait_named_pipe, create_file, set_pipe_state, (read_file, write_file, close_handle)


def windows_pipe_exchange(endpoint: str, payload: bytes, timeout: float) -> bytes:
    """Connect to one private benchmark pipe instance and read one JSON line."""
    wait_named_pipe, create_file, set_pipe_state, io_api = _windows_pipe_api()
    read_file, write_file, close_handle = io_api
    deadline = time.monotonic() + timeout
    handle: int | None = None
    while time.monotonic() < deadline:
        if wait_named_pipe(endpoint, 100):
            handle = create_file(endpoint, 0xC0000000, 0, None, 3, 0, None)
            if handle not in (None, ctypes.c_void_p(-1).value):
                break
        else:
            error = ctypes.get_last_error()
            # ERROR_SEM_TIMEOUT means no instance is currently available;
            # ERROR_FILE_NOT_FOUND means startup has not created it yet.
            if error not in (2, 121):
                raise ctypes.WinError(error)
    if handle in (None, ctypes.c_void_p(-1).value):
        raise TimeoutError("named-pipe instance did not become available")
    try:
        # Nonblocking byte mode lets this probe enforce its own bounded read
        # deadline without creating worker threads that might outlive a run.
        mode = ctypes.c_uint32(0x00000001)  # PIPE_READMODE_BYTE is zero; PIPE_NOWAIT is one.
        if not set_pipe_state(handle, ctypes.byref(mode), None, None):
            raise ctypes.WinError(ctypes.get_last_error())
        payload_buffer = ctypes.create_string_buffer(payload)
        written = ctypes.c_uint32()
        if not write_file(handle, payload_buffer, len(payload), ctypes.byref(written), None):
            raise ctypes.WinError(ctypes.get_last_error())
        if written.value != len(payload):
            raise OSError("named-pipe request frame was only partially written")
        result = bytearray()
        while time.monotonic() < deadline:
            chunk = ctypes.create_string_buffer(min(4096, MAX_OUTPUT + 1 - len(result)))
            count = ctypes.c_uint32()
            if read_file(handle, chunk, len(chunk), ctypes.byref(count), None):
                result.extend(chunk.raw[:count.value])
                if b"\n" in result:
                    return bytes(result[: result.index(b"\n") + 1])
                if len(result) > MAX_OUTPUT:
                    return bytes(result)
            else:
                error = ctypes.get_last_error()
                if error not in (109, 232, 234, 536, 997):  # broken/no data/listening/overlapped
                    raise ctypes.WinError(error)
                time.sleep(0.005)
        raise TimeoutError("named-pipe response exceeded probe deadline")
    finally:
        close_handle(handle)


def daemon_exchange(endpoint: str | Path, frame: dict[str, object], timeout: float) -> bytes:
    payload = (json.dumps(frame) + "\n").encode()
    if os.name == "nt":
        return windows_pipe_exchange(str(endpoint), payload, timeout)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(timeout)
        connection.connect(str(endpoint))
        connection.sendall(payload)
        return connection.recv(MAX_OUTPUT + 1)


def endpoint_ready(endpoint: str | Path) -> bool:
    if os.name == "nt":
        wait_named_pipe, _, _, _ = _windows_pipe_api()
        return bool(wait_named_pipe(str(endpoint), 25))
    return Path(endpoint).exists()


def daemon_ping_until_ready(endpoint: str | Path, session: str, process: subprocess.Popen[bytes], deadline: float) -> bytes:
    """Retry only the startup window where an endpoint exists before accept is ready."""
    last_error: OSError | None = None
    while time.monotonic() < deadline:
        try:
            return daemon_exchange(endpoint, {"cmd": "daemon.ping", "session": session},
                                  max(0.05, deadline - time.monotonic()))
        except OSError as error:
            last_error = error
            if process.poll() is not None:
                break
            time.sleep(0.01)
    if last_error is not None:
        raise last_error
    raise TimeoutError("daemon did not accept a ping before the startup deadline")


def daemon_probe(
    binary: Path, env: dict[str, str], root: Path, runs: int, *, static_mode: bool
) -> dict[str, object]:
    if os.name == "nt" and not static_mode:
        return {
            "status": "unsupported",
            "reason": "Go daemon binds Unix sockets; only the Rust candidate exposes the native Windows named pipe",
        }
    if os.name != "posix" and os.name != "nt":
        return {"status": "unsupported", "reason": "daemon probe requires Unix sockets or Windows named pipes"}
    results: list[dict[str, object]] = []
    session = probe_session("rust016")
    endpoint = daemon_endpoint(session, env)
    for _ in range(runs):
        process, stderr_path = launch_daemon(daemon_command(binary, session, static_mode=static_mode), root, env)
        started = time.perf_counter_ns()
        try:
            startup_deadline = time.monotonic() + 5
            while time.monotonic() < startup_deadline and not endpoint_ready(endpoint):
                if process.poll() is not None:
                    break
                time.sleep(0.02)
            if not endpoint_ready(endpoint):
                results.append(startup_failure(process, "daemon endpoint did not appear", stderr_path))
                continue
            response = daemon_ping_until_ready(endpoint, session, process, startup_deadline)
            if b'"success":true' not in response:
                results.append({"status": "error", "reason": "daemon ping failed"})
            else:
                results.append({"status": "pass", "duration_ns": time.perf_counter_ns() - started})
            daemon_exchange(endpoint, {"cmd": "daemon.stop", "session": session}, 3)
            process.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired) as error:
            results.append({"status": "error", "reason": str(error)})
        finally:
            terminate_process_tree(process)
            stderr_path.unlink(missing_ok=True)
    return summarize(results)


def fetch_semantics(response: bytes, expected_url: str) -> tuple[bool, str]:
    """Validate the complete static-fetch contract, not just success:true."""
    try:
        frame = json.loads(response.decode("utf-8"))
        data = frame.get("data", frame)
        meta = data.get("meta", {})
        body = data.get("content", data.get("markdown"))
        title = data.get("title", meta.get("title", ""))
        document_ok = (
            data.get("final_url", meta.get("final_url")) == expected_url
            and data.get("status_code", meta.get("status_code")) == 200
            and title == "RUST-016"
            and isinstance(body, str)
            and "fixture" in body
            and meta.get("final_url") == expected_url
            and meta.get("status_code") == 200
            and meta.get("protocol", "HTTP/1.1") in {"HTTP/1.1", "HTTP/2.0"}
        )
        return document_ok, "exact response/document metadata" if document_ok else "response semantics mismatch"
    except (UnicodeDecodeError, json.JSONDecodeError, AttributeError, TypeError):
        return False, "invalid JSON response"


def negative_control_rejected(expected_url: str) -> bool:
    """Ensure a fast success-shaped but wrong document cannot pass."""
    wrong = {"success": True, "data": {"final_url": expected_url, "status_code": 200,
                                      "title": "wrong", "content": "wrong", "meta": {
                                          "final_url": expected_url, "status_code": 200,
                                          "title": "wrong", "protocol": "HTTP/1.1"}}}
    accepted, _ = fetch_semantics(json.dumps(wrong).encode(), expected_url)
    return not accepted


def fetch_probe(
    binary: Path, env: dict[str, str], root: Path, runs: int, *, static_mode: bool
) -> dict[str, object]:
    if os.name == "nt" and not static_mode:
        return {
            "status": "unsupported",
            "reason": "Go fetch daemon requires its Unix-socket transport; Windows named-pipe fetch is Rust-only",
        }
    if os.name != "posix" and os.name != "nt":
        return {"status": "unsupported", "reason": "fetch daemon probe requires Unix sockets or Windows named pipes"}
    server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    session = probe_session("rust016-fetch")
    endpoint = daemon_endpoint(session, env)
    process, stderr_path = launch_daemon(daemon_command(binary, session, static_mode=static_mode), root, env)
    try:
        startup_deadline = time.monotonic() + 5
        while time.monotonic() < startup_deadline and not endpoint_ready(endpoint):
            if process.poll() is not None:
                break
            time.sleep(0.02)
        if not endpoint_ready(endpoint):
            return startup_failure(process, "fetch daemon endpoint did not appear", stderr_path)
        if b'"success":true' not in daemon_ping_until_ready(
            endpoint, session, process, startup_deadline
        ):
            return {"status": "error", "reason": "fetch daemon readiness ping failed"}
        samples = []
        for index in range(runs):
            started = time.perf_counter_ns()
            frame = {
                "cmd": "fetch.url",
                "session": session,
                "args": {
                    "url": f"http://127.0.0.1:{server.server_port}/fixture.html?run={index}",
                    "no_cache": True,
                },
            }
            try:
                response = daemon_exchange(endpoint, frame, 15)
                duration = time.perf_counter_ns() - started
                if len(response) > MAX_OUTPUT:
                    samples.append({"status": "error", "reason": "output limit exceeded"})
                elif b'"success":true' not in response:
                    samples.append({"status": "error", "reason": "fetch daemon request failed"})
                else:
                    semantic_pass, reason = fetch_semantics(response, frame["args"]["url"])
                    samples.append({
                        "status": "pass" if semantic_pass else "error",
                        "duration_ns": duration,
                        "semantic_check": reason,
                    })
            except OSError as error:
                samples.append({"status": "error", "reason": str(error)})
        result = summarize(samples)
        result["semantic_contract"] = {
            "fixture_id": "rust016-static-fetch-html-v1",
            "checks": ["final_url", "status_code", "body", "document_metadata", "errors"],
            "negative_control": {
                "candidate": "success=true with wrong title/body",
                "rejected": negative_control_rejected(
                    f"http://127.0.0.1:{server.server_port}/fixture.html?run=negative"
                ),
            },
        }
        return result
    finally:
        try:
            if endpoint_ready(endpoint):
                daemon_exchange(endpoint, {"cmd": "daemon.stop", "session": session}, 3)
        except OSError:
            pass
        terminate_process_tree(process)
        stderr_path.unlink(missing_ok=True)
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()


def binary_identity(binary: Path, repo_root: Path) -> dict[str, object]:
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    try:
        revision = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=repo_root, text=True, stderr=subprocess.DEVNULL
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        revision = "unknown"
    return {
        "path": str(binary),
        "size_bytes": binary.stat().st_size,
        "sha256": digest,
        "vcs_revision": revision,
    }


def run_binary(
    binary: Path,
    selected: Collection[str],
    runs: int,
    root: Path,
    repo_root: Path,
    *,
    static_mode: bool,
) -> dict[str, object]:
    if not binary.is_file() or not os.access(binary, os.X_OK):
        return {"status": "blocked", "reason": f"missing or non-executable binary: {binary}"}
    env = implementation_env(root, "rust" if static_mode else "go")
    probes = {
        "cli": Probe("cli", ("version", "--json")),
        "mcp": Probe(
            "mcp",
            ("mcp", "--engine", "static"),
            '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"rust016","version":"0"}}}\n'
            '{"jsonrpc":"2.0","id":2,"method":"tools/list"}\n',
        ),
    }
    result: dict[str, object] = {"identity": binary_identity(binary, repo_root)}
    for name, probe in probes.items():
        if name in selected:
            result[name] = summarize([run_once(binary, probe, env, root) for _ in range(runs)])
    if "daemon" in selected:
        result["daemon"] = daemon_probe(binary, env, root, runs, static_mode=static_mode)
    if "fetch" in selected:
        result["fetch"] = fetch_probe(binary, env, root, runs, static_mode=static_mode)
    return result


def main(argv: Sequence[str] | None = None) -> int:
    ensure_external_environment(__file__)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", type=Path)
    parser.add_argument("--rust", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--workload", action="append", choices=WORKLOADS)
    parser.add_argument("--strict", action="store_true", help="fail if either binary lacks a selected workload")
    args = parser.parse_args(argv)
    if args.runs < 1 or args.runs > 100:
        parser.error("--runs must be between 1 and 100")
    selected = set(args.workload or WORKLOADS)
    output = external_output(args.output)
    with tempfile.TemporaryDirectory(prefix="b-", dir=temporary_parent()) as raw:
        root = Path(raw)
        report: dict[str, Any] = {
            "schema_version": 2,
            "report_version": "rust016-benchmark-v2",
            "captured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "source_revision": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=Path(__file__).resolve().parents[2], text=True
            ).strip(),
            "host": {"system": platform.system(), "machine": platform.machine()},
            "daemon_transport": "windows-named-pipe" if os.name == "nt" else "unix-domain-socket",
            "daemon_session_policy": "randomized per probe invocation to avoid user or concurrent daemon collisions",
            "runs_per_workload": args.runs,
            "cache_policy": "no_cache=true for fetch requests; fresh HOME/XDG roots per process probe",
            "workload_fixture": "rust016-static-fetch-html-v1",
            "workloads": sorted(selected),
            "p95_calculation": "nearest-rank: sorted_samples[ceil(0.95*n)-1]",
            "binaries": {},
            "gate": "blocked",
            "limitations": [
                "Peak RSS is collected from child-process resource usage where the host exposes it; daemon RSS remains unavailable in this portable runner.",
                "The fetch probe is a local HTTP fixture and does not certify real browser/CDP behavior.",
                "Unsupported candidate surfaces remain a BLOCK for cutover, not a passing result.",
                *(
                    ["The Go daemon exposes Unix sockets only; Windows native-pipe samples are Rust-only and cannot satisfy the paired RUST-016 gate."]
                    if os.name == "nt"
                    else []
                ),
            ],
        }
        for name, path in (("go", args.go), ("rust", args.rust)):
            if path is None:
                report["binaries"][name] = {"status": "blocked", "reason": "binary argument not supplied"}
                continue
            report["binaries"][name] = run_binary(
                path.resolve(),
                selected,
                args.runs,
                root,
                Path(__file__).resolve().parents[2],
                static_mode=name == "rust",
            )
        rust_result = report["binaries"].get("rust")
        if isinstance(rust_result, dict):
            if isinstance(rust_result.get("identity"), dict) and isinstance(rust_result["identity"].get("size_bytes"), int):
                report["candidate_size_bytes"] = rust_result["identity"]["size_bytes"]
            rss_values = [
                workload["median_peak_rss_bytes"]
                for name, workload in rust_result.items()
                if name in selected
                and isinstance(workload, dict)
                and isinstance(workload.get("median_peak_rss_bytes"), int)
            ]
            if rss_values:
                report["candidate_median_peak_rss_bytes"] = int(statistics.median(rss_values))
        go_result = report["binaries"].get("go")
        if isinstance(go_result, dict) and isinstance(go_result.get("identity"), dict) and isinstance(go_result["identity"].get("size_bytes"), int):
            report["reference_size_bytes"] = go_result["identity"]["size_bytes"]
        all_pass = True
        for binary_result in report["binaries"].values():
            if not isinstance(binary_result, dict) or binary_result.get("status") == "blocked":
                all_pass = False
                continue
            for workload in selected:
                if not isinstance(binary_result.get(workload), dict) or binary_result[workload].get("status") != "pass":
                    all_pass = False
        report["gate"] = "pass" if all_pass else "blocked"
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report, indent=2))
        if args.strict and not all_pass:
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
