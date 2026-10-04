#!/usr/bin/env python3
"""Recover real MCP oracle output and exercise the production daemon proxy."""
from __future__ import annotations

import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("daemon_process", HERE / "daemon_process.py")
assert SPEC and SPEC.loader
process_harness = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(process_harness)
harness = process_harness.harness
BROWSE = HERE.parents[1]
MANIFEST = BROWSE / "testdata/port/mcp/manifest.json"


def command(binary: Path, session: str, name: str) -> list[str]:
    args = [str(binary), "mcp", "--session", session]
    if name == "tools_nav":
        args += ["--tools", "nav"]
    elif name == "tools_all":
        args += ["--tools", "all"]
    return args


def encode(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def start(binary: Path, session: str, env: dict, root: Path):
    daemon = subprocess.Popen([str(binary), "daemon", "--session", session, "--ssrf"],
                              env=env, cwd=root, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, start_new_session=(os.name == "posix"))
    endpoint = harness.daemon_socket_path(Path(env["XDG_RUNTIME_DIR"]), session)
    try:
        harness.wait_for_request(endpoint, {"cmd": "daemon.ping", "session": session})
    except Exception:
        harness.kill_tree(daemon)
        stdout, stderr = daemon.communicate(timeout=3)
        raise AssertionError(f"daemon startup failed: {daemon.returncode}, {stdout!r}, {stderr!r}")
    return daemon, endpoint


def stop(daemon, endpoint, session: str):
    try:
        harness.request(endpoint, {"cmd": "daemon.stop", "session": session})
        stdout, stderr = daemon.communicate(timeout=10)
        if daemon.returncode != 0 or stdout:
            raise AssertionError(f"daemon shutdown failed: {daemon.returncode}, {stdout!r}, {stderr!r}")
    finally:
        harness.kill_tree(daemon)


def controls(cargo: str, tests: dict, fixture: Path, endpoint: Path) -> list[dict]:
    original = (fixture / "initialize.out").read_bytes()
    results = []
    for name, test_name, diagnostic in (
        ("missing-output", "initialize_is_byte_exact_and_clean", "read MCP output fixture"),
        ("changed-output", "initialize_is_byte_exact_and_clean", "fixture=initialize"),
        ("missing-daemon", "malformed_unknown_and_typed_argument_errors_match", "fixture=tool_error"),
    ):
        environment = dict(tests)
        if name == "missing-output":
            (fixture / "initialize.out").unlink()
        elif name == "changed-output":
            (fixture / "initialize.out").write_bytes(b"X" + original[1:])
        else:
            environment["SYMBROWSE_MCP_DAEMON_ENDPOINT"] = str(endpoint) + ".absent"
        try:
            result = subprocess.run(
                [cargo, "test", "-p", "symbrowse-mcp", "--test", "raw_frames", test_name, "--locked"],
                cwd=BROWSE, env=environment, capture_output=True, timeout=90)
        finally:
            (fixture / "initialize.out").write_bytes(original)
        log = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        if result.returncode == 0 or diagnostic not in log or "1 failed" not in log:
            raise AssertionError(f"fixture failure control did not reject {name}: {log}")
        results.append({"name": name, "exit": result.returncode,
                        "intended_diagnostic": diagnostic, "rejected": True})
    return results


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--go", type=Path, required=True, help="Go CLI built with version dev")
    parser.add_argument("--go-fixture", type=Path, required=True, help="Same Go source, version v0.8.0")
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--go-source", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--workspace", action="store_true")
    args = parser.parse_args()
    source = args.go_source.resolve()
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    if revision != process_harness.GO_REF or dirty:
        raise AssertionError("MCP oracle source must be immutable dcddcef0")
    manifest = json.loads(MANIFEST.read_text())
    source_hashes = {name: process_harness.digest(source / "browse" / name)
                     for name in manifest["source_files"]}
    if source_hashes != manifest["source_files"]:
        raise AssertionError("current Go MCP source differs from historical source manifest")
    if len(manifest["fixtures"]) != 13:
        raise AssertionError("historical MCP manifest must contain all thirteen cases")
    report = {"go_ref": revision, "historical_manifest_sha256": process_harness.digest(MANIFEST),
              "historical_mcp_source_hashes_match": True, "source_hashes": source_hashes,
              "binaries_sha256": {name: process_harness.digest(getattr(args, name))
                                  for name in ("go", "go_fixture", "rust")}, "observations": []}
    with tempfile.TemporaryDirectory(prefix="bm-", dir=process_harness.private_temporary_parent()) as directory:
        root = Path(directory); home = root / "home"; home.mkdir(mode=0o700)
        runtime = home / "Library/Caches/symbrowse/run" if sys.platform == "darwin" else root / "run"
        runtime.mkdir(parents=True, mode=0o700)
        env = {key: os.environ[key] for key in ("SystemRoot", "windir", "ComSpec", "PATHEXT", "TMP", "TEMP")
               if key in os.environ}
        env.update(HOME=str(home), USERPROFILE=str(home), LOCALAPPDATA=str(root / "Local"),
                   XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                   XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"),
                   XDG_RUNTIME_DIR=str(runtime), PATH="", SYMBROWSE_NO_AUTOSTART="1")
        session = f"mcp-parity-{os.getpid()}"
        fixture = root / "fixture"; fixture.mkdir(mode=0o700)
        daemon, endpoint = start(args.go.resolve(), session, env, root)
        try:
            for name, paths in manifest["fixtures"].items():
                data = (MANIFEST.parent / paths["input"]).read_bytes()
                frozen = subprocess.run(command(args.go_fixture.resolve(), session, name), input=data,
                                        env=env, cwd=root, capture_output=True, timeout=15, check=True)
                live = subprocess.run(command(args.go.resolve(), session, name), input=data,
                                      env=env, cwd=root, capture_output=True, timeout=15, check=True)
                if frozen.stderr or live.stderr:
                    raise AssertionError(f"Go MCP stderr pollution: {name}")
                (fixture / paths["input"]).write_bytes(data)
                (fixture / paths["output"]).write_bytes(frozen.stdout)
                report["observations"].append({"name": name, "input_base64": encode(data),
                                               "fixture_go_stdout_base64": encode(frozen.stdout),
                                               "go_stdout_base64": encode(live.stdout)})
        finally:
            stop(daemon, endpoint, session)
        daemon, endpoint = start(args.rust.resolve(), session, env, root)
        try:
            for record in report["observations"]:
                result = subprocess.run(command(args.rust.resolve(), session, record["name"]),
                                        input=base64.b64decode(record["input_base64"]), env=env,
                                        cwd=root, capture_output=True, timeout=15)
                record.update(rust_exit=result.returncode, rust_stdout_base64=encode(result.stdout),
                              rust_stderr_base64=encode(result.stderr))
                record["matches"] = result.returncode == 0 and not result.stderr and \
                    record["rust_stdout_base64"] == record["go_stdout_base64"]
            # Keep Cargo/Rustup in the existing toolchain roots while routing
            # test-created state and browser profiles into this owned HOME.
            tests = {k: v for k, v in os.environ.items() if not k.startswith("SYMBROWSE_")}
            tests["CARGO_HOME"] = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))
            tests["RUSTUP_HOME"] = os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup"))
            tests.update({k: v for k, v in env.items() if k != "PATH"})
            tests.update(SYMBROWSE_MCP_DAEMON_ENDPOINT=str(endpoint),
                         SYMBROWSE_MCP_ORACLE_FIXTURE_DIR=str(fixture), SYMBROWSE_MCP_ORACLE_SESSION=session)
            cargo = shutil.which("cargo")
            if not cargo:
                raise AssertionError("Cargo is required for the actual fixture assertion")
            gate = [cargo, "test", "--workspace", "--locked"] if args.workspace else \
                [cargo, "test", "-p", "symbrowse-mcp", "--test", "raw_frames", "--locked"]
            tested = subprocess.run(gate, cwd=BROWSE, env=tests, capture_output=True, timeout=1800)
            Path(str(args.out) + ".tests.log").write_bytes(tested.stdout + tested.stderr)
            report["test_exit"] = tested.returncode
            report["negative_controls"] = controls(cargo, tests, fixture, endpoint)
        finally:
            stop(daemon, endpoint, session)
    report["total"] = len(report["observations"])
    report["matches"] = report["total"] == 13 and all(r["matches"] for r in report["observations"])
    report["candidate_head"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=BROWSE, text=True).strip()
    report["candidate_dirty"] = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=BROWSE))
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"13 actual MCP fixtures: matches={report['matches']}; Rust test exit={report['test_exit']}")
    return 0 if report["matches"] and report["test_exit"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
