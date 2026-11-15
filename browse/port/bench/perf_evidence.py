#!/usr/bin/env python3
"""Rust-only PERF-001..PERF-004 measurements against the frozen Go baseline.

Go was removed in #1003, so nothing here runs Go. Each workload is measured
with the same argv and iteration count (30 runs) as `docs/rust-port/baseline.json`
(`measurements.rust_001_portbench`), plus a 5-run warmup that is not reported.
Per-process peak RSS comes from `wait4` rusage of exactly that child (never the
cumulative RUSAGE_CHILDREN maximum). The measurement is repeated for several
rounds so run-to-run variance on a shared host is visible; every raw sample is
kept. Steady-state probes drive one long-lived `mcp` and one `daemon` process
through 100 frames and sample RSS and thread count before/after.

Only loopback fixtures are used; no public network access happens.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

WARMUP = 5
RUNS = 30
FRAMES = 100
MCP_STDIN = (
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05",'
    '"capabilities":{},"clientInfo":{"name":"perf","version":"0"}}}\n'
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}\n'
)
WORKLOADS = {
    # name: (argv, stdin)  -- names and argv mirror baseline.json rust_001_portbench
    "version-json": (("version", "--json"), ""),
    "root-help": (("--help",), ""),
    "config-show-json": (("config", "show", "--json"), ""),
    "mcp-initialize-list": (("mcp", "--engine", "static"), MCP_STDIN),
}
BODY = b"<html><head><title>RUST-016</title></head><body><main><p>fixture</p></main></body></html>\n"


def p95(values: list[float]) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(0.95 * len(ordered)) - 1)]


def summarize(durations: list[int], rss: list[int]) -> dict:
    out = {
        "samples": len(durations),
        "min_ns": min(durations),
        "median_ns": int(statistics.median(durations)),
        "p95_ns": int(p95(durations)),
        "max_ns": max(durations),
        "raw_duration_ns": durations,
    }
    if rss:
        out.update(median_rss_bytes=int(statistics.median(rss)), max_rss_bytes=max(rss),
                   raw_rss_bytes=rss)
    return out


def env_for(root: Path) -> dict[str, str]:
    home = root / "home"
    for name in ("home", "runtime", "cache", "tmp"):
        (root / name).mkdir(mode=0o700, exist_ok=True)
    env = {
        "HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"),
        "XDG_DATA_HOME": str(home / ".local" / "share"), "XDG_CACHE_HOME": str(root / "cache"),
        "XDG_STATE_HOME": str(home / ".local" / "state"), "XDG_RUNTIME_DIR": str(root / "runtime"),
        "TMPDIR": str(root / "tmp"), "LANG": "C", "LC_ALL": "C", "TZ": "UTC", "TERM": "dumb",
        "NO_COLOR": "1", "SYMBROWSE_CHECK_UPDATES": "0", "SYMBROWSE_SYMGUARD": "off",
        "SYMBROWSE_ALLOW_PRIVATE": "true", "SYMBROWSE_MODE": "browser",
        "PATH": os.environ.get("PATH", ""),
    }
    return env


def run_once(binary: Path, argv: tuple[str, ...], stdin_text: str, env: dict, root: Path) -> tuple[int, int]:
    """Return (wall_ns, peak_rss_bytes) for one process; raise on a bad run."""
    stdin_path = root / "tmp" / "stdin.txt"
    stdin_path.write_text(stdin_text)
    out_path = root / "tmp" / "stdout.txt"
    with stdin_path.open("rb") as sin, out_path.open("wb") as sout:
        started = time.perf_counter_ns()
        proc = subprocess.Popen([str(binary), *argv], stdin=sin, stdout=sout,
                                stderr=subprocess.DEVNULL, env=env, cwd=root)
        _, status, usage = os.wait4(proc.pid, 0)
        wall = time.perf_counter_ns() - started
    proc.returncode = 0  # reaped by wait4
    if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0:
        raise RuntimeError(f"{argv}: exit status {status}")
    out = out_path.read_bytes()
    if not out:
        raise RuntimeError(f"{argv}: empty stdout")
    if argv and argv[0] == "mcp" and not (b'"id":1' in out and b'"id":2' in out and b'"tools"' in out):
        raise RuntimeError("mcp: initialize/tools-list response incomplete")
    rss = usage.ru_maxrss if platform.system() == "Darwin" else usage.ru_maxrss * 1024
    return wall, rss


def process_round(binary: Path, root: Path) -> dict:
    env = env_for(root)
    result = {}
    # Control: a trivial native tool through the identical spawn/wait4 path. It
    # bounds the harness + host scheduling floor so contention is visible.
    controls = {"control-echo": (Path("/bin/echo"), ("x",), "")}
    plan = [(name, binary, argv, stdin_text) for name, (argv, stdin_text) in WORKLOADS.items()]
    plan += [(name, exe, argv, stdin_text) for name, (exe, argv, stdin_text) in controls.items()]
    for name, exe, argv, stdin_text in plan:
        for _ in range(WARMUP):
            run_once(exe, argv, stdin_text, env, root)
        durations, rss = [], []
        for _ in range(RUNS):
            wall, peak = run_once(exe, argv, stdin_text, env, root)
            durations.append(wall)
            rss.append(peak)
        result[name] = summarize(durations, rss)
    return result


def ps_stats(pid: int) -> dict:
    rss_kb = int(subprocess.check_output(["ps", "-o", "rss=", "-p", str(pid)], text=True).strip())
    threads = len(subprocess.check_output(["ps", "-M", "-p", str(pid)], text=True).strip().splitlines()) - 1
    fds = len([line for line in subprocess.run(["lsof", "-p", str(pid)], capture_output=True, text=True).stdout.splitlines()[1:]])
    return {"rss_bytes": rss_kb * 1024, "threads": threads, "open_fds": fds}


class Fixture(BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(BODY)))
        self.end_headers()
        self.wfile.write(BODY)

    def log_message(self, *_args: object) -> None:
        return


def frame_summary(durations: list[int]) -> dict:
    return {"frames": len(durations), "median_ns": int(statistics.median(durations)),
            "p95_ns": int(p95(durations)), "max_ns": max(durations), "raw_duration_ns": durations}


def daemon_pid_for(sock: Path, exclude: int) -> int:
    """PID of the process serving `sock` (the daemon the mcp server auto-spawned)."""
    out = subprocess.run(["lsof", "-t", str(sock)], capture_output=True, text=True).stdout.split()
    pids = [int(value) for value in out if int(value) != exclude and int(value) != os.getpid()]
    if not pids:
        raise RuntimeError(f"no process serves {sock}")
    return pids[0]


def mcp_steady(binary: Path, root: Path, port: int) -> dict:
    """One long-lived `mcp` process driving its own auto-spawned static-capable daemon.

    This is the product flow: `tools/call` makes the mcp server start (or reuse) the
    session daemon. fetch_batch is a pure HTTP fetch, so no browser is launched.
    """
    env = env_for(root)
    proc = subprocess.Popen([str(binary), "mcp"], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, cwd=root, bufsize=0)
    sock = Path(env["HOME"]) / "Library" / "Caches" / "symbrowse" / "run" / "default.sock"

    def rpc(payload: dict) -> bytes:
        proc.stdin.write((json.dumps(payload) + "\n").encode())
        return proc.stdout.readline()

    daemon_pid = None
    try:
        t0 = time.perf_counter_ns()
        init = rpc({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "perf", "version": "0"}}})
        init_ns = time.perf_counter_ns() - t0
        t0 = time.perf_counter_ns()
        listing = rpc({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        list_ns = time.perf_counter_ns() - t0
        assert b'"serverInfo"' in init and b'"tools"' in listing

        def call(index: int) -> bytes:
            url = f"http://127.0.0.1:{port}/fixture.html?mcp={index}"
            return rpc({"jsonrpc": "2.0", "id": 5000 + index, "method": "tools/call", "params": {
                "name": "fetch_batch", "arguments": {"urls": [url]}}})

        t0 = time.perf_counter_ns()
        first = call(0)  # includes daemon auto-spawn
        first_call_ns = time.perf_counter_ns() - t0
        assert b"RUST-016" in first, first[:300]
        for index in range(1, WARMUP):
            call(index)
        daemon_pid = daemon_pid_for(sock, proc.pid)
        before = {"mcp": ps_stats(proc.pid), "daemon": ps_stats(daemon_pid)}
        pings, calls, call_ok = [], [], 0
        for index in range(FRAMES):
            t0 = time.perf_counter_ns()
            rep = rpc({"jsonrpc": "2.0", "id": 1000 + index, "method": "ping"})
            pings.append(time.perf_counter_ns() - t0)
            assert b'"result"' in rep
        for index in range(FRAMES):
            t0 = time.perf_counter_ns()
            rep = call(100 + index)
            calls.append(time.perf_counter_ns() - t0)
            call_ok += int(b"RUST-016" in rep and b'"isError":false' in rep)
        after = {"mcp": ps_stats(proc.pid), "daemon": ps_stats(daemon_pid)}
        return {"initialize_ns": init_ns, "tools_list_ns": list_ns,
                "first_tools_call_incl_daemon_spawn_ns": first_call_ns,
                "ping": frame_summary(pings), "tools_call_fetch_batch": frame_summary(calls),
                "tools_call_semantic_ok": call_ok, "before": before, "after": after}
    finally:
        proc.kill()
        proc.wait()
        if daemon_pid is not None:
            try:
                os.kill(daemon_pid, 15)
            except ProcessLookupError:
                pass


def daemon_steady(binary: Path, root: Path, port: int) -> dict:
    """One long-lived static daemon: FRAMES daemon.ping + FRAMES fetch.url on one session."""
    env = env_for(root)
    session = "perf"
    sock_path = root / "runtime" / "symbrowse" / f"{session}.sock"
    alt = Path(env["HOME"]) / "Library" / "Caches" / "symbrowse" / "run" / f"{session}.sock"
    proc = subprocess.Popen([str(binary), "daemon", "--session", session, "--mode", "static"],
                            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, env=env, cwd=root, start_new_session=True)

    def send(frame: dict) -> bytes:
        path = sock_path if sock_path.exists() else alt
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
            conn.settimeout(15)
            conn.connect(str(path))
            conn.sendall((json.dumps(frame) + "\n").encode())
            data = b""
            while not data.endswith(b"\n"):
                chunk = conn.recv(1 << 16)
                if not chunk:
                    break
                data += chunk
            return data

    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and not (sock_path.exists() or alt.exists()):
            if proc.poll() is not None:
                raise RuntimeError("daemon exited before creating its socket")
            time.sleep(0.02)
        for index in range(WARMUP):
            send({"cmd": "daemon.ping", "session": session})
        before = ps_stats(proc.pid)
        pings, fetches, ok = [], [], 0
        for _ in range(FRAMES):
            t0 = time.perf_counter_ns()
            rep = send({"cmd": "daemon.ping", "session": session})
            pings.append(time.perf_counter_ns() - t0)
            assert b'"success":true' in rep
        for index in range(FRAMES):
            url = f"http://127.0.0.1:{port}/fixture.html?d={index}"
            t0 = time.perf_counter_ns()
            rep = send({"cmd": "fetch.url", "session": session, "args": {"url": url, "no_cache": True}})
            fetches.append(time.perf_counter_ns() - t0)
            ok += int(b'"success":true' in rep and b"RUST-016" in rep)
        after = ps_stats(proc.pid)
        send({"cmd": "daemon.stop", "session": session})
        proc.wait(timeout=10)
        return {"ping": frame_summary(pings), "fetch_url": frame_summary(fetches),
                "fetch_semantic_ok": ok, "before": before, "after": after}
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()


def sh(*cmd: str) -> str:
    try:
        return subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        return "unavailable"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--base", type=Path, default=None, help="short scratch dir for sockets")
    args = parser.parse_args()
    binary = args.rust.resolve(strict=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    rounds = []
    base = str(args.base) if args.base else None
    for number in range(args.rounds):
        with tempfile.TemporaryDirectory(prefix="pe-", dir=base) as raw:
            root = Path(raw)
            load_start = os.getloadavg()[0]
            started = time.strftime("%Y-%m-%dT%H:%M:%S%z")
            process = process_round(binary, root)
            mcp = mcp_steady(binary, root, server.server_port)
            daemon = daemon_steady(binary, root, server.server_port)
            rounds.append({"round": number + 1, "started": started, "load1_at_start": load_start,
                           "load1_at_end": os.getloadavg()[0], "process": process,
                           "mcp_steady": mcp, "daemon_steady": daemon})
    server.shutdown()
    report = {
        "schema_version": 1,
        "report_version": "perf-evidence-v1",
        "captured_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "binary": {"path": str(binary), "size_bytes": binary.stat().st_size,
                   "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()},
        "warmup_runs": WARMUP, "runs_per_workload": RUNS, "steady_state_frames": FRAMES,
        "p95_calculation": "nearest-rank: sorted_samples[ceil(0.95*n)-1]",
        "rss_source": "wait4 ru_maxrss of the measured child only (bytes on Darwin)",
        "rounds": rounds,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
