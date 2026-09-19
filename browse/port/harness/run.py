#!/usr/bin/env python3
"""Run bounded, hermetic Go-oracle migration suites."""
from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import time
from pathlib import Path

MAX_FRAME_BYTES = 1 << 20
EXTERNAL_BASE_ENV = "SYMAIRA_EXTERNAL_BASE"
EXTERNAL_RUNTIME_ENV = "SYMAIRA_EXTERNAL_RUNTIME_ROOT"
EXTERNAL_RUNTIME_ROOT = Path("/Volumes/1TB_NVMe_SN850X")
DEFAULT_EXTERNAL_BASE = EXTERNAL_RUNTIME_ROOT / "Dev" / "Symaira_Dev" / "builds" / "symaira-brain"


def _external_path(value: str | Path, mounted_root: Path, *, name: str, create: bool = False) -> Path:
    path = Path(value).expanduser()
    if not path.is_absolute():
        raise RuntimeError(f"{name} must be an absolute path under {mounted_root}: {path}")
    try:
        resolved = path.resolve(strict=not create)
    except OSError as error:
        raise RuntimeError(f"{name} is invalid: {path}: {error}") from error
    if not resolved.is_relative_to(mounted_root):
        raise RuntimeError(f"{name} must be under mounted NVMe volume {mounted_root}: {resolved}")
    if create:
        path.mkdir(parents=True, exist_ok=True)
        resolved = path.resolve(strict=True)
    if not resolved.is_dir() or not os.access(resolved, os.W_OK | os.X_OK):
        raise RuntimeError(f"{name} must name a writable directory: {resolved}")
    return resolved


def external_environment(env: dict[str, str]) -> dict[str, str]:
    """Keep direct macOS build, temp, and evidence paths on the encrypted NVMe."""
    if sys.platform != "darwin" or env.get("CI"):
        return env
    try:
        mounted_root = EXTERNAL_RUNTIME_ROOT.resolve(strict=True)
    except OSError as error:
        raise RuntimeError(f"required NVMe runtime volume is unavailable: {EXTERNAL_RUNTIME_ROOT}: {error}") from error
    if not mounted_root.is_dir() or not mounted_root.is_mount():
        raise RuntimeError(f"required NVMe runtime volume is not mounted: {mounted_root}")
    base = _external_path(
        env.get(EXTERNAL_BASE_ENV, str(DEFAULT_EXTERNAL_BASE)),
        mounted_root,
        name=EXTERNAL_BASE_ENV,
        create=True,
    )
    runtime = _external_path(
        env.get(EXTERNAL_RUNTIME_ENV, str(EXTERNAL_RUNTIME_ROOT / "tmp")),
        mounted_root,
        name=EXTERNAL_RUNTIME_ENV,
        create=True,
    )
    paths = {
        "TMPDIR": base / "tmp",
        "TMP": base / "tmp",
        "TEMP": base / "tmp",
        "GOTMPDIR": base / "go-tmp",
        "GOPATH": base / "gopath",
        "GOCACHE": base / "go-cache",
        "GOMODCACHE": base / "go-mod-cache",
        "FETCH_GO_CACHE": base / "go-cache",
        "FETCH_GO_MODCACHE": base / "go-mod-cache",
        "GOTELEMETRYDIR": base / "go-telemetry",
        "CARGO_HOME": base / "cargo-home",
        "CARGO_TARGET_DIR": base / "cargo-target",
        "PYTHONPYCACHEPREFIX": base / "python-cache",
        EXTERNAL_BASE_ENV: base,
        EXTERNAL_RUNTIME_ENV: runtime,
    }
    for name, path in paths.items():
        paths[name] = _external_path(path, mounted_root, name=name, create=True)
        env[name] = str(paths[name])
    return env


def temporary_parent(env: dict[str, str] | None = None) -> str | None:
    selected = os.environ if env is None else env
    if sys.platform != "darwin" or selected.get("CI"):
        return None
    return external_environment(selected)[EXTERNAL_RUNTIME_ENV]


def run(command: list[str], root: Path, env: dict[str, str], *, timeout: int = 600) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=root, env=env, check=True, timeout=timeout)


def wait_for_path(path: Path, timeout: float = 5.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.02)
    raise AssertionError(f"timed out waiting for {path}")


def kill_tree(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGTERM)
        else:
            process.terminate()
        process.wait(timeout=2)
    except (OSError, subprocess.TimeoutExpired):
        try:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
        except OSError:
            pass
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass


def daemon_socket_path(runtime: Path, session: str) -> Path:
    if os.name == "nt":
        return Path(r"\\.\pipe") / f"symbrowse-{session}"
    if sys.platform == "darwin":
        return runtime / f"{session}.sock"
    return runtime / "symbrowse" / f"{session}.sock"


def start_daemon(binary: Path, env: dict[str, str], session: str) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [str(binary), "daemon", "--session", session],
        cwd=binary.parent.parent.parent,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=(os.name == "posix"),
    )


def request(socket_path: Path, frame: dict[str, object], *, timeout: float = 3.0) -> dict[str, object]:
    payload = json.dumps(frame, separators=(",", ":")).encode() + b"\n"
    if len(payload) >= MAX_FRAME_BYTES:
        raise ValueError("harness request must stay below the daemon frame limit")
    if os.name == "nt":
        # Python exposes named pipes as byte streams on Windows. Opening the
        # endpoint for each request mirrors the Rust client's one-request
        # connection lifecycle and needs no third-party package.
        with open(socket_path, "r+b", buffering=0) as connection:
            connection.write(payload)
            response = bytearray()
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline and len(response) <= MAX_FRAME_BYTES:
                chunk = connection.read(min(65536, MAX_FRAME_BYTES + 1 - len(response)))
                if not chunk:
                    break
                response.extend(chunk)
                if b"\n" in response:
                    return json.loads(bytes(response).split(b"\n", 1)[0])
        raise AssertionError("daemon closed without a JSON response")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(timeout)
        connection.connect(str(socket_path))
        connection.sendall(payload)
        response = bytearray()
        while len(response) <= MAX_FRAME_BYTES:
            chunk = connection.recv(min(65536, MAX_FRAME_BYTES + 1 - len(response)))
            if not chunk:
                break
            response.extend(chunk)
            if b"\n" in response:
                return json.loads(bytes(response).split(b"\n", 1)[0])
    raise AssertionError("daemon closed without a JSON response")


def wait_for_request(
    socket_path: Path,
    frame: dict[str, object],
    *,
    timeout: float = 5.0,
) -> dict[str, object]:
    deadline = time.monotonic() + timeout
    last_error: OSError | None = None
    while time.monotonic() < deadline:
        try:
            return request(socket_path, frame)
        except (ConnectionRefusedError, FileNotFoundError, TimeoutError, socket.timeout) as error:
            last_error = error
            time.sleep(0.02)
    raise AssertionError(f"timed out waiting for daemon response: {last_error}")


def assert_clean_process(process: subprocess.Popen[bytes], *, timeout: float = 5.0) -> None:
    stdout, stderr = process.communicate(timeout=timeout)
    if stdout:
        raise AssertionError(f"daemon wrote to stdout: {stdout[:200]!r}")
    if len(stderr) > 65536:
        raise AssertionError("daemon stderr exceeded harness output bound")
    lowered = stderr.lower()
    if b"password=" in lowered or b"token=" in lowered:
        raise AssertionError("daemon stderr leaked a secret-like value")


def cargo_target_root(root: Path, env: dict[str, str]) -> Path:
    value = env.get("CARGO_TARGET_DIR", "").strip()
    if not value:
        if sys.platform == "darwin" and not env.get("CI"):
            return Path(external_environment(env)["CARGO_TARGET_DIR"])
        return root / "target"
    target = Path(value)
    if sys.platform == "darwin" and not env.get("CI") and not target.is_absolute():
        raise RuntimeError("CARGO_TARGET_DIR must be an absolute NVMe path on direct macOS runs")
    return target if target.is_absolute() else root / target


def external_output(root: Path, env: dict[str, str], relative: str) -> Path:
    if sys.platform == "darwin" and not env.get("CI"):
        return Path(env[EXTERNAL_BASE_ENV]) / relative
    return root / relative


def external_file(path: Path, env: dict[str, str], *, name: str) -> Path:
    """Reject direct macOS evidence paths that escape the external volume."""
    if sys.platform != "darwin" or env.get("CI"):
        return path
    mounted_root = EXTERNAL_RUNTIME_ROOT.resolve(strict=True)
    resolved = path.expanduser().resolve(strict=False)
    if not resolved.is_relative_to(mounted_root):
        raise RuntimeError(f"{name} must be under mounted NVMe volume {mounted_root}: {resolved}")
    return resolved


def lifecycle_once(binary: Path, env: dict[str, str], runtime: Path, *, suffix: str) -> None:
    session = f"contract-{suffix}"
    socket_path = daemon_socket_path(runtime, session)
    process = start_daemon(binary, env, session)
    try:
        if os.name == "nt":
            # A named pipe is not visible through Path.exists().
            wait_for_request(socket_path, {"cmd": "daemon.ping", "session": session})
        else:
            wait_for_path(socket_path)
        if os.name == "nt":
            # Named pipes have no filesystem mode bits; access is enforced by
            # the owner-only DACL installed by the Rust listener.
            pass
        else:
            mode = stat.S_IMODE(socket_path.stat().st_mode)
            if mode != 0o600:
                raise AssertionError(f"socket mode is {mode:o}, expected 600")
        status = request(socket_path, {"cmd": "daemon.status", "session": session})
        status_data = status.get("data")
        if not status.get("success") or not isinstance(status_data, dict) or not status_data.get("running"):
            raise AssertionError(f"unexpected daemon.status response: {status}")
        listed = request(socket_path, {"cmd": "session.list", "session": session})
        list_data = listed.get("data")
        sessions = list_data.get("sessions", []) if isinstance(list_data, dict) else []
        if not any(isinstance(item, dict) and item.get("name") == session for item in sessions):
            raise AssertionError(f"session registry omitted {session}: {listed}")
        if os.name != "nt":
            oversized = b"{" + b"x" * MAX_FRAME_BYTES + b"}\n"
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
                connection.settimeout(2)
                connection.connect(str(socket_path))
                try:
                    connection.sendall(oversized)
                except BrokenPipeError:
                    pass
                else:
                    if connection.recv(1):
                        raise AssertionError("oversized daemon frame received a response")
        stop = request(socket_path, {"cmd": "daemon.stop", "session": session})
        if not stop.get("success"):
            raise AssertionError(f"unexpected daemon.stop response: {stop}")
        process.wait(timeout=5)
        if socket_path.exists():
            raise AssertionError("daemon socket survived clean shutdown")
    finally:
        kill_tree(process)
        if os.name != "nt" and socket_path.exists():
            raise AssertionError("daemon socket survived harness cleanup")
    assert_clean_process(process)


def stale_socket_once(binary: Path, env: dict[str, str], runtime: Path, *, suffix: str) -> None:
    if os.name == "nt":
        # Named-pipe instances disappear with their owning process; the
        # crash-safe mutex is the stale-endpoint recovery mechanism.
        return
    session = f"stale-{suffix}"
    socket_dir = runtime if sys.platform == "darwin" else runtime / "symbrowse"
    socket_dir.mkdir(mode=0o700, exist_ok=True)
    socket_path = socket_dir / f"{session}.sock"
    stale = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    stale.bind(str(socket_path))
    stale.close()
    process = start_daemon(binary, env, session)
    try:
        stop = wait_for_request(
            socket_path,
            {"cmd": "daemon.stop", "session": session},
        )
        if not stop.get("success"):
            raise AssertionError(f"unexpected daemon.stop response: {stop}")
        process.wait(timeout=5)
    finally:
        kill_tree(process)
    assert_clean_process(process)
    if os.name != "nt" and socket_path.exists():
        raise AssertionError("stale-socket recovery left the socket behind")


def race_once(binary: Path, env: dict[str, str], runtime: Path, *, suffix: str, starters: int) -> None:
    session = f"race-{suffix}"
    socket_path = daemon_socket_path(runtime, session)
    processes = [start_daemon(binary, env, session) for _ in range(starters)]
    try:
        if os.name == "nt":
            wait_for_request(socket_path, {"cmd": "daemon.ping", "session": session}, timeout=10.0)
        else:
            wait_for_path(socket_path, timeout=10.0)
        status = request(socket_path, {"cmd": "daemon.status", "session": session})
        if not status.get("success"):
            raise AssertionError(f"race daemon did not become queryable: {status}")
        request(socket_path, {"cmd": "daemon.stop", "session": session})
        for process in processes:
            process.wait(timeout=10)
        for process in processes:
            assert_clean_process(process)
    finally:
        for process in processes:
            kill_tree(process)
    if os.name != "nt" and socket_path.exists():
        raise AssertionError("race cleanup left a live socket")


def daemon_suite(root: Path, env: dict[str, str], *, rounds: int, starters: int) -> None:
    binary_value = env.get("SYMBROWSE_RUST_BINARY")
    if binary_value:
        binary = Path(binary_value)
        if not binary.is_file():
            raise AssertionError(f"installed Rust daemon binary does not exist: {binary}")
    else:
        run(["cargo", "build", "-p", "symbrowse-cli", "--locked"], root, env)
        binary = cargo_target_root(root, env) / "debug" / (
            "symbrowse.exe" if os.name == "nt" else "symbrowse"
        )
    # macOS limits Unix-domain socket paths to 104 bytes. Keep this root short
    # without silently putting runner state on the local system volume.
    with tempfile.TemporaryDirectory(prefix="sb-", dir=temporary_parent(env)) as directory:
        base = Path(directory)
        data = base / "data"
        home = base / "home"
        for path in (data, home):
            path.mkdir(mode=0o700)
        runtime = (
            home / "Library" / "Caches" / "symbrowse" / "run"
            if sys.platform == "darwin"
            else base / "runtime"
        )
        runtime.mkdir(parents=True, mode=0o700)
        scoped = dict(
            env,
            HOME=str(home),
            XDG_RUNTIME_DIR=str(runtime),
            SYMBROWSE_USER_DATA_DIR=str(data),
            SYMBROWSE_NO_AUTOSTART="1",
        )
        lifecycle_once(binary, scoped, runtime, suffix="one")
        stale_socket_once(binary, scoped, runtime, suffix="one")
        for index in range(rounds):
            race_once(binary, scoped, runtime, suffix=str(index), starters=starters)
    print(f"daemon suite passed ({rounds} rounds x {starters} starters)", flush=True)


FETCH_CONTROL_CASE_IDS = (
    "FETCH-001-profile-selection",
    "FETCH-003-http-semantics",
    "FETCH-004-redirect-proxy-cookie",
    "FETCH-005-robots-retry-rate-limit",
    "FETCH-009-static-selection",
    "FETCH-009-browser-unavailable",
    "FETCH-009-compat-unavailable",
    "FETCH-010-static-honesty",
)


ALL_SUITES = (
    "engine-neutral",
    "fetch-control",
    "fetch-fingerprints",
    "fetch-render",
    "workflows",
    "daemon",
    "chrome-spike",
    "chrome-full",
    "safari",
    "browser-transport",
    "compat-sidecar",
)


def run_all_suites(root: Path, env: dict[str, str], args: argparse.Namespace) -> None:
    for suite in ALL_SUITES:
        command = [sys.executable, str(Path(__file__).resolve()), "--suite", suite]
        if suite == "fetch-fingerprints":
            rust_binary = args.rust or (
                Path(env["SYMBROWSE_RUST_BINARY"])
                if env.get("SYMBROWSE_RUST_BINARY")
                else None
            )
            compat_binary = args.compat or (
                Path(env["SYMBROWSE_COMPAT_BINARY"])
                if env.get("SYMBROWSE_COMPAT_BINARY")
                else None
            )
            command.extend([
                "--capture", str(args.capture.resolve()),
                "--trusted-sha256", args.trusted_sha256,
            ])
            if rust_binary is not None and compat_binary is not None:
                command.extend(["--rust", str(rust_binary.resolve()), "--compat", str(compat_binary.resolve())])
            if args.historical_oracle:
                command.extend(["--historical-oracle", str(args.historical_oracle.resolve())])
            if args.report:
                command.extend(["--report", str(args.report.resolve())])
            if args.runtime_root:
                command.extend(["--runtime-root", str(args.runtime_root.resolve())])
            command.extend(["--go", args.go, "--repo-root", str(root.parent.resolve())])
        if args.comparison != "bytes" and suite == "fetch-render":
            command.extend(["--comparison", args.comparison])
        if args.native_targets and suite in {"chrome-full", "safari"}:
            command.extend(["--native", "macos"])
        run(command, root, env, timeout=1800)
    if args.native_targets:
        run(
            ["cargo", "test", "--workspace", "--all-targets", "--locked"],
            root,
            env,
            timeout=1800,
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", required=True)
    parser.add_argument("--repeat", type=int, default=50)
    parser.add_argument("--comparison", choices=("bytes", "json-semantic", "filesystem"), default="bytes")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--capture", type=Path)
    parser.add_argument("--trusted-sha256")
    parser.add_argument("--historical-oracle", type=Path)
    parser.add_argument("--rust", type=Path, help="retained Rust candidate binary for FETCH-002")
    parser.add_argument("--compat", type=Path, help="retained Go compat-sidecar binary for FETCH-002")
    parser.add_argument("--report", type=Path, help="FETCH-002 comparison report path")
    parser.add_argument(
        "--runtime-root",
        type=Path,
        help="short encrypted-NVMe directory for the daemon runtime socket",
    )
    parser.add_argument("--go", default="go")
    parser.add_argument("--native", choices=("macos",), default=None)
    parser.add_argument(
        "--native-targets",
        action="store_true",
        help="run real installed-browser gates and write machine-readable evidence",
    )
    args = parser.parse_args()
    if args.native and args.native_targets:
        parser.error("use either --native macos or --native-targets, not both")
    native = args.native_targets or args.native == "macos"
    if args.repeat < 1 or args.repeat > 1000:
        parser.error("--repeat must be between 1 and 1000")
    root = Path(__file__).resolve().parents[2]
    env = os.environ.copy()
    env.update({"CGO_ENABLED": "0", "GOTOOLCHAIN": "go1.26.6"})
    external_environment(env)
    if args.suite == "all":
        if not args.capture or not args.trusted_sha256:
            parser.error("--suite all needs FETCH-002 --capture and --trusted-sha256 artifacts")
        run_all_suites(root, env, args)
        print("all harness suites passed")
        return 0

    if args.suite == "engine-neutral":
        commands = [
            [sys.executable, "scripts/rust-port/engine_fixture_gen.py", "--check"],
            ["cargo", "nextest", "run", "-p", "symbrowse-engine", "--locked"],
            ["cargo", "test", "-p", "symbrowse-engine", "--doc", "--locked"],
        ]
    elif args.suite == "fetch-control":
        commands = [
            ["go", "run", "./scripts/rust-port/fetch_fixture_gen.go", "--check"],
            ["go", "run", "-tags", "rustport", "./scripts/rust-port/cmd/fetchcontrolgen", "--check"],
            ["go", "test", "./internal/fetch/..."],
            ["cargo", "test", "-p", "symbrowse-fetch", "--all-targets", "--locked"],
        ]
    elif args.suite == "fetch-render":
        if args.comparison != "bytes":
            parser.error("fetch-render only supports --comparison bytes")
        commands = [
            ["go", "run", "./scripts/rust-port/fetch_fixture_gen.go", "--check"],
            ["go", "test", "./internal/fetch/dom/...", "./internal/fetch/render/...", "./internal/fetch/relevance/...", "./internal/fetch/semantic/..."],
            ["cargo", "test", "-p", "symbrowse-fetch", "--test", "render_corpus", "--test", "static_controls", "--test", "pipeline_controls", "--locked"],
        ]
    elif args.suite == "browser-transport":
        fixture = json.loads((root / "testdata/port/core/transport-selection.json").read_text())
        assert fixture["schema_version"] == 1 and len(fixture["cases"]) == 8
        assert {case["id"] for case in fixture["cases"]} == {
            "static-default", "browser-chrome", "browser-safari", "browser-firefox",
            "browser-missing-engine", "static-engine-conflict", "unknown-mode", "unknown-engine",
        }
        commands = [["cargo", "test", "-p", "symbrowse-core", "selection_is_exhaustive", "--locked"]]
    elif args.suite == "fetch-fingerprints":
        # The capture, historical oracle and both binaries are retained artifacts.
        # This suite never builds or self-approves any of them.
        if not args.capture or not args.trusted_sha256:
            parser.error("fetch-fingerprints needs --capture and an independently reviewed --trusted-sha256")
        rust_binary = args.rust or (Path(os.environ["SYMBROWSE_RUST_BINARY"]) if os.environ.get("SYMBROWSE_RUST_BINARY") else None)
        compat_binary = args.compat or (Path(os.environ["SYMBROWSE_COMPAT_BINARY"]) if os.environ.get("SYMBROWSE_COMPAT_BINARY") else None)
        if (rust_binary is None) != (compat_binary is None):
            parser.error("fetch-fingerprints needs Rust and Go compat artifacts together when supplied")
        oracle = args.historical_oracle or (root / "docs/rust-port/rust009-tls-results.json")
        report = external_file(
            args.report or external_output(root, env, "target/fetch002-next/fetch002-e2e.json"),
            env,
            name="--report",
        )
        command = [sys.executable, "port/harness/fetch_fingerprints_validate.py",
             "--capture", str(args.capture.resolve()),
             "--trusted-sha256", args.trusted_sha256,
             "--historical-oracle", str(oracle.resolve()),
             "--report", str(report.resolve()),
             "--repo-root", str(root.parent.resolve()),
             "--go", args.go]
        if rust_binary is not None and compat_binary is not None:
            command.extend(["--rust", str(rust_binary.resolve()), "--compat", str(compat_binary.resolve())])
        runtime_root = args.runtime_root or (
            Path(env[EXTERNAL_RUNTIME_ENV]) if sys.platform == "darwin" and not env.get("CI") else None
        )
        if runtime_root:
            command.extend(["--runtime-root", str(runtime_root.resolve())])
        run(command, root, env)
        print("FETCH-002 diagnostics completed; acceptance remains blocked")
        return 0
    elif args.suite == "compat-sidecar":
        fixture = json.loads((root / "port/harness/cases/compat-sidecar.json").read_text())
        assert fixture["schema_version"] == 1 and len(fixture["cases"]) == 8
        commands = [
            ["go", "build", "-trimpath", "-o", str(external_output(root, env, "dist/symbrowse-compat")), "./cmd/symbrowse"],
            ["cargo", "test", "-p", "symbrowse-compat", "-p", "symbrowse-daemon", "--locked"],
        ]
    elif args.suite == "workflows":
        commands = [
            ["go", "run", "./scripts/rust-port/cmd/workflowgen", "--check"],
            ["cargo", "test", "-p", "symbrowse-core", "--test", "workflows_contract", "--locked"],
        ]
    elif args.suite == "chrome-spike":
        commands = [
            ["cargo", "test", "-p", "symbrowse-engine-chrome", "--test", "spike", "--locked"],
        ]
    elif args.suite in {"chrome-full", "safari"}:
        suites = (args.suite,)
        for suite in suites:
            fixture = [
                sys.executable,
                "scripts/rust-port/browser_fixture_gen.py",
                "--suite",
                suite,
                "--check",
            ]
            if native:
                fixture.extend(["--native", "macos"])
            run(fixture, root, env)
            package = "symbrowse-engine-chrome" if suite == "chrome-full" else "symbrowse-engine-safari"
            run(["cargo", "test", "-p", package, "--test", "contract_fixture", "--locked"], root, env)
        print(f"{args.suite} suite passed")
        return 0
    elif args.suite == "daemon":
        daemon_suite(root, env, rounds=1, starters=1)
        return 0
    elif args.suite == "daemon-races":
        daemon_suite(root, env, rounds=args.repeat, starters=50)
        return 0
    else:
        parser.error(f"unsupported suite: {args.suite}")

    for command in commands:
        run(command, root, env)
    if args.suite == "fetch-control":
        print("executed fetch-control case IDs: " + ",".join(FETCH_CONTROL_CASE_IDS), flush=True)
    print(f"{args.suite} suite passed ({args.comparison})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
