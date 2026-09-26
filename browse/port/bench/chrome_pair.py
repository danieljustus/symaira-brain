#!/usr/bin/env python3
"""Measure a paired Go/Rust real-Chrome local-fixture flow on one native host."""
from __future__ import annotations

import argparse
import errno
import hashlib
import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Sequence

FIXTURE_TITLE = "PERF-003 Chrome fixture"
FIXTURE_TOKEN = "chrome-fixture-token-9281"
P95_LIMIT = 1.10
MAX_OUTPUT = 1 << 20
METADATA_URL = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json"
TARGETS = {
    "darwin-amd64": ("Darwin", {"x86_64", "amd64"}),
    "darwin-arm64": ("Darwin", {"arm64", "aarch64"}),
    "linux-amd64": ("Linux", {"x86_64", "amd64"}),
    "linux-arm64": ("Linux", {"aarch64", "arm64"}),
    "windows-amd64": ("Windows", {"amd64", "x86_64"}),
    "windows-arm64": ("Windows", {"arm64", "aarch64"}),
}
SUPPORTED_CFT = set(TARGETS) - {"windows-arm64"}


def remove_owned_tempdir(
    root: Path,
    *,
    timeout: float = 5.0,
    remove=shutil.rmtree,
    sleep=time.sleep,
) -> None:
    """Retry removal only for this runner-owned profile after browser shutdown."""
    deadline = time.monotonic() + timeout
    while root.exists():
        try:
            remove(root)
            return
        except FileNotFoundError:
            return
        except OSError as error:
            if error.errno not in {errno.ENOTEMPTY, errno.EBUSY, errno.EACCES, errno.EPERM}:
                raise
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise
            sleep(min(0.05, remaining))


class FixtureHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802 - stdlib callback
        body = (
            "<!doctype html><html><head><title>" + FIXTURE_TITLE + "</title></head>"
            "<body><main><h1>" + FIXTURE_TITLE + "</h1><p>" + FIXTURE_TOKEN + "</p></main></body></html>"
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        return


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def nearest_rank(samples: list[int]) -> int:
    ordered = sorted(samples)
    return ordered[max(0, (len(ordered) * 95 + 99) // 100 - 1)]


def summarize_stage(samples: list[dict[str, Any]], field: str) -> dict[str, int | str]:
    values = [
        int(sample[field])
        for sample in samples
        if sample.get("status") == "pass" and isinstance(sample.get(field), int)
    ]
    if not values:
        return {"status": "incomplete", "samples": 0}
    return {
        "status": "complete" if len(values) == len(samples) else "incomplete",
        "samples": len(values),
        "median_duration_ns": int(statistics.median(values)),
        "p95_duration_ns": nearest_rank(values),
    }


def paired_gate_passes(go_samples: list[int], rust_samples: list[int], required: int) -> bool:
    return (
        len(go_samples) == required
        and len(rust_samples) == required
        and bool(go_samples)
        and nearest_rank(rust_samples) <= nearest_rank(go_samples) * P95_LIMIT
    )


def native_target_matches(target: str, system: str, machine: str) -> bool:
    expected = TARGETS.get(target)
    return bool(expected and expected[0] == system and machine.casefold() in expected[1])


def make_env(root: Path, implementation: str, chrome: Path, launcher: Path | None = None) -> dict[str, str]:
    home = root / "home"
    tmp = root / "tmp"
    for path in (home, tmp, root / "runtime", root / "cache"):
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith("SYMBROWSE_")}
    for key in ("PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "USER", "USERNAME"):
        if key in os.environ:
            env[key] = os.environ[key]
    executable = launcher or chrome
    env.update({
        "HOME": str(home), "USERPROFILE": str(home),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "XDG_DATA_HOME": str(home / ".local" / "share"),
        "XDG_CACHE_HOME": str(root / "cache"),
        "XDG_STATE_HOME": str(home / ".local" / "state"),
        "XDG_RUNTIME_DIR": str(root / "runtime"),
        "TMPDIR": str(tmp), "TMP": str(tmp), "TEMP": str(tmp),
        "SYMBROWSE_MODE": "browser", "SYMBROWSE_ENGINE": "chrome",
        "SYMBROWSE_EXECUTABLE_PATH": str(executable),
        "SYMBROWSE_CHROME_EXECUTABLE": str(executable),
        "SYMBROWSE_HEADLESS": "1",
        "SYMBROWSE_CHECK_UPDATES": "0", "SYMBROWSE_SYMGUARD": "off",
        "SYMBROWSE_ALLOW_PRIVATE": "true", "LANG": "C", "LC_ALL": "C", "TZ": "UTC",
        "NO_COLOR": "1",
    })
    if os.name == "nt":
        env["APPDATA"] = str(home / "AppData" / "Roaming")
        env["LOCALAPPDATA"] = str(home / "AppData" / "Local")
    if implementation == "go":
        env["SYMBROWSE_ENGINE"] = "chrome"
    elif implementation == "rust":
        env["SYMBROWSE_MODE"] = "browser"
    else:
        raise ValueError(f"unknown implementation: {implementation}")
    return env


def command(binary: Path, args: list[str], session: str) -> list[str]:
    if args[:2] in (["daemon", "stop"], ["daemon", "status"]):
        return [str(binary), "--json", "daemon", args[1], "--session", session, *args[2:]]
    return [str(binary), "--json", args[0], "--session", session, *args[1:]]


def run_cli(
    binary: Path,
    args: list[str],
    session: str,
    env: dict[str, str],
    cwd: Path,
    *,
    timeout: float = 45,
) -> tuple[int, str, str]:
    result = subprocess.run(
        command(binary, args, session), cwd=cwd, env=env, capture_output=True,
        text=True, timeout=timeout, check=False,
    )
    if len(result.stdout) > MAX_OUTPUT or len(result.stderr) > MAX_OUTPUT:
        return result.returncode or 1, "", "output limit exceeded"
    return result.returncode, result.stdout, result.stderr


def validate_read_output(output: str) -> bool:
    if not output:
        return False
    try:
        document: Any = json.loads(output)
    except json.JSONDecodeError:
        return False
    serialized = json.dumps(document, sort_keys=True).casefold()
    return FIXTURE_TITLE.casefold() in serialized and FIXTURE_TOKEN.casefold() in serialized


def wait_for_daemon_exit(
    binary: Path,
    session: str,
    env: dict[str, str],
    cwd: Path,
    *,
    timeout: float = 10.0,
    sleep=time.sleep,
) -> bool:
    """Wait until daemon.stop's asynchronous server teardown has completed."""
    deadline = time.monotonic() + timeout
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return False
        try:
            code, stdout, _ = run_cli(
                binary, ["daemon", "status"], session, env, cwd,
                timeout=min(1.0, remaining),
            )
        except subprocess.TimeoutExpired:
            sleep(min(0.05, max(0.0, deadline - time.monotonic())))
            continue
        except OSError:
            return False
        if code != 0:
            return True
        try:
            document = json.loads(stdout)
        except json.JSONDecodeError:
            return False
        data = document.get("data") if isinstance(document, dict) else None
        if isinstance(data, dict) and data.get("running") is False:
            return True
        sleep(min(0.05, max(0.0, deadline - time.monotonic())))


def flow(binary: Path, implementation: str, chrome: Path, launcher: Path | None, url: str, root: Path, index: int) -> dict[str, Any]:
    session = f"p3-{implementation[0]}-{index}-{os.getpid()}"
    env = make_env(root, implementation, chrome, launcher)
    started = time.perf_counter_ns()
    outcome: dict[str, Any] = {"status": "error", "phase": "startup"}
    try:
        open_started = time.perf_counter_ns()
        opened = run_cli(binary, ["open", url], session, env, root)
        open_duration = time.perf_counter_ns() - open_started
        if opened[0] != 0:
            outcome = {"status": "error", "phase": "open", "exit_code": opened[0],
                       "open_cli_duration_ns": open_duration,
                       "stdout_sha256": hashlib.sha256(opened[1].encode()).hexdigest(),
                       "stderr_sha256": hashlib.sha256(opened[2].encode()).hexdigest()}
            try:
                error = json.loads(opened[1]).get("error", {})
                if isinstance(error, dict):
                    outcome["error_code"] = str(error.get("code", ""))[:128]
                    outcome["error_message"] = str(error.get("message", ""))[:256]
            except (json.JSONDecodeError, AttributeError):
                pass
        else:
            read_started = time.perf_counter_ns()
            read = run_cli(binary, ["read"], session, env, root)
            read_duration = time.perf_counter_ns() - read_started
            elapsed = time.perf_counter_ns() - started
            if read[0] != 0 or not validate_read_output(read[1]):
                outcome = {"status": "error", "phase": "read", "exit_code": read[0],
                           "semantic_contract": "local fixture title/token must appear in JSON read output",
                           "stdout_sha256": hashlib.sha256(read[1].encode()).hexdigest(),
                           "stderr_sha256": hashlib.sha256(read[2].encode()).hexdigest(), "duration_ns": elapsed,
                           "open_cli_duration_ns": open_duration, "read_cli_duration_ns": read_duration}
            else:
                outcome = {"status": "pass", "duration_ns": elapsed,
                           "open_cli_duration_ns": open_duration, "read_cli_duration_ns": read_duration}
    except (OSError, subprocess.TimeoutExpired) as error:
        outcome = {"status": "error", "reason": type(error).__name__, "duration_ns": time.perf_counter_ns() - started}
    finally:
        try:
            run_cli(binary, ["daemon", "stop"], session, env, root)
            if not wait_for_daemon_exit(binary, session, env, root):
                if outcome["status"] == "pass":
                    outcome = {"status": "error", "phase": "daemon-stop",
                               "reason": "daemon did not exit within 10 seconds after stop"}
                else:
                    outcome["cleanup_error"] = "daemon did not exit within 10 seconds after stop"
        except (OSError, subprocess.TimeoutExpired):
            if outcome["status"] == "pass":
                outcome = {"status": "error", "phase": "daemon-stop",
                           "reason": "daemon shutdown could not be confirmed"}
            else:
                outcome["cleanup_error"] = "daemon shutdown could not be confirmed"
    return outcome


def identity(binary: Path) -> dict[str, Any]:
    return {"sha256": sha256(binary), "size_bytes": binary.stat().st_size}


def measure(args: argparse.Namespace) -> dict[str, Any]:
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=args.repo, capture_output=True, text=True,
        check=True,
    ).stdout.strip()
    report: dict[str, Any] = {
        "schema_version": 1, "report_version": "perf003-real-chrome-paired-v1",
        "target": args.target, "source_revision": revision,
        "host": {"system": platform.system(), "machine": platform.machine()}, "status": "blocked",
        "fixture": {"id": "perf003-local-chrome-html-v1", "title": FIXTURE_TITLE,
                    "content_token": FIXTURE_TOKEN, "flow": ["open", "read"]},
        "chrome": {"provider": "Chrome for Testing", "version": args.chrome_version,
                   "metadata_url": METADATA_URL, "archive_sha256": args.chrome_archive_sha256},
        "binaries": {}, "runs_per_implementation": args.runs,
        "p95_calculation": "nearest-rank: sorted_samples[ceil(0.95*n)-1]",
        "gate": "blocked", "reason": None,
    }
    if args.expected_source_revision and revision != args.expected_source_revision:
        report["reason"] = "checkout source revision differs from expected workflow SHA"
        return report
    if not native_target_matches(args.target, platform.system(), platform.machine()):
        report["reason"] = f"native runner identity mismatch: {platform.system()}/{platform.machine()}"
        return report
    if not all(path.is_file() and os.access(path, os.X_OK) for path in (args.go, args.rust)):
        report["reason"] = "both native Go and Rust binaries must be executable files"
        return report
    report["binaries"] = {"go": identity(args.go), "rust": identity(args.rust)}
    if args.target not in SUPPORTED_CFT:
        report["status"] = "unsupported"
        report["reason"] = "official Chrome for Testing has no Windows ARM64 browser artifact"
        report["chrome"]["version"] = None
        report["chrome"]["archive_sha256"] = None
        return report
    if not args.chrome or not args.chrome.is_file() or not args.chrome_version or not args.chrome_archive_sha256:
        report["reason"] = "verified Chrome for Testing executable/version/archive digest are required"
        return report
    if len(args.chrome_archive_sha256) != 64 or any(char not in "0123456789abcdef" for char in args.chrome_archive_sha256):
        report["reason"] = "Chrome for Testing archive SHA-256 must be 64 lowercase hexadecimal characters"
        return report
    report["chrome"]["binary_sha256"] = sha256(args.chrome)

    server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{server.server_port}/fixture.html"
    samples: dict[str, list[dict[str, Any]]] = {"go": [], "rust": []}
    chooser = random.Random(int(revision[:12], 16))
    try:
        for index in range(args.runs):
            order = ["go", "rust"]
            if chooser.randrange(2):
                order.reverse()
            for implementation in order:
                binary = args.go if implementation == "go" else args.rust
                temp = Path(tempfile.mkdtemp(prefix=f"p3-{implementation[0]}-", dir="/tmp" if sys.platform == "darwin" else None))
                try:
                    samples[implementation].append(
                        flow(binary, implementation, args.chrome, args.chrome_launcher, url, temp, index)
                    )
                finally:
                    remove_owned_tempdir(temp)
            if any(samples[implementation][-1]["status"] != "pass" for implementation in ("go", "rust")):
                break
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    for implementation in ("go", "rust"):
        passing = [int(item["duration_ns"]) for item in samples[implementation] if item.get("status") == "pass"]
        report["binaries"][implementation]["chrome_flow"] = {
            "status": "pass" if len(passing) == args.runs else "error",
            "samples": samples[implementation], "sample_count": len(passing),
            "p95_duration_ns": nearest_rank(passing) if passing else None,
            "median_duration_ns": int(statistics.median(passing)) if passing else None,
            "stages": {
                "open_cli": summarize_stage(samples[implementation], "open_cli_duration_ns"),
                "read_cli": summarize_stage(samples[implementation], "read_cli_duration_ns"),
            },
        }
    go_p95 = report["binaries"]["go"]["chrome_flow"]["p95_duration_ns"]
    rust_p95 = report["binaries"]["rust"]["chrome_flow"]["p95_duration_ns"]
    report["gate"] = "pass" if paired_gate_passes(
        [int(item["duration_ns"]) for item in samples["go"] if item.get("status") == "pass"],
        [int(item["duration_ns"]) for item in samples["rust"] if item.get("status") == "pass"],
        args.runs,
    ) else "blocked"
    report["status"] = "measured" if go_p95 is not None and rust_p95 is not None else "error"
    if report["gate"] != "pass":
        report["reason"] = "paired Chrome flow failed or Rust p95 exceeds 110% of Go"
    return report


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--chrome", type=Path)
    parser.add_argument("--chrome-launcher", type=Path)
    parser.add_argument("--chrome-version")
    parser.add_argument("--chrome-archive-sha256")
    parser.add_argument("--expected-source-revision")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--runs", type=int, default=30)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.runs < 1:
        parser.error("--runs must be positive")
    report = measure(args)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if report.get("gate") == "pass" or report.get("status") == "unsupported" else 1


if __name__ == "__main__":
    raise SystemExit(main())
