#!/usr/bin/env python3
"""Supplemental process parity against the immutable integrated Go daemon.

The historical 652453d fixtures remain the migration baseline. This runner
adds current dcddcef0 observations; it does not replace those fixtures.
"""
from __future__ import annotations

import argparse
import copy
from datetime import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("daemon_harness", HERE / "run.py")
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(harness)
GO_REF = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
COMMANDS = ("daemon.ping", "daemon.status", "session.list", "session.info",
            "session.ensure", "unknown", "daemon.stop")
SETTINGS = ({}, {"SYMBROWSE_ALLOW_PRIVATE": "true"},
            {"SYMBROWSE_SSRF": "true"}, {"XDG_CACHE_HOME": "relative-cache"},
            {"LOCALAPPDATA": ""})


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def observe(binary: Path, settings: dict[str, str], session: str,
            raw_origin: bytes | None = None, git_origin: bool = False) -> dict:
    with tempfile.TemporaryDirectory(prefix="bd-", dir=harness.temporary_parent(os.environ)) as temporary:
        root = Path(temporary)
        home = root / "home"
        home.mkdir(mode=0o700)
        temporary = root / "temp"
        temporary.mkdir(mode=0o700)
        runtime = home / "Library/Caches/symbrowse/run" if sys.platform == "darwin" else root / "run"
        runtime.mkdir(parents=True, mode=0o700)
        # Keep platform launch requirements while clearing all inherited policy.
        env = {key: os.environ[key] for key in
               ("SystemRoot", "windir", "ComSpec", "PATHEXT", "TMP", "TEMP") if key in os.environ}
        env.update(HOME=str(home), USERPROFILE=str(home), LOCALAPPDATA=str(root / "Local"),
                   XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                   XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"),
                   XDG_RUNTIME_DIR=str(runtime), PATH="", SYMBROWSE_NO_AUTOSTART="1",
                   TMPDIR=str(temporary), TMP=str(temporary), TEMP=str(temporary), **settings)
        cwd = root
        if raw_origin is not None:
            cwd = root / os.fsdecode(b"origin-" + raw_origin)
            cwd.mkdir(mode=0o700)
        if git_origin:
            # A private POSIX child emits raw git stdout. Require the recorded
            # origin below to prove that both production probes executed it.
            git_bin = root / "bin"
            git_bin.mkdir(mode=0o700)
            script = git_bin / "git"
            payload = os.fsencode(root) + b"/git-result-\xe2\x82\n"
            escaped = "".join(f"\\{byte:03o}" for byte in payload)
            script.write_text(f"#!/bin/sh\nprintf '{escaped}'\n")
            script.chmod(0o700)
            env["PATH"] = str(git_bin)
        begin = time.time()
        process = subprocess.Popen([str(binary), "daemon", "--session", session], cwd=cwd,
                                   env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=(os.name == "posix"))
        endpoint = harness.daemon_socket_path(runtime, session)
        observations = []
        try:
            harness.wait_for_request(endpoint, {"cmd": "daemon.ping", "session": session})
            for command in COMMANDS:
                frame = {"cmd": command, "session": session}
                observations.append({"request": frame, "response": harness.request(endpoint, frame)})
            stdout, stderr = process.communicate(timeout=10)
            if process.returncode != 0 or stdout:
                raise AssertionError(f"daemon exit/stdout: {process.returncode}, {stdout!r}, {stderr!r}")
            if git_origin:
                origin = observations[3]["response"]["data"]["origin_path"]
                if origin != str(root) + "/git-result-\ufffd\ufffd":
                    raise AssertionError(f"production git probe did not retain raw stdout: {origin!r}")
        except Exception:
            harness.kill_tree(process)
            stdout, stderr = process.communicate(timeout=3)
            print(f"failed daemon: settings={settings}; exit={process.returncode}; "
                  f"stdout={stdout!r}; stderr={stderr!r}", file=sys.stderr)
            raise
        finally:
            harness.kill_tree(process)
        return {"root": str(root), "endpoint": str(endpoint), "pid": process.pid,
                "begin": begin, "end": time.time(), "exit": process.returncode,
                "stdout_hex": stdout.hex(), "stderr_hex": stderr.hex(), "observations": observations}


def normalized(observation: dict, *, rust: bool, session: str, settings: dict) -> list:
    records = observation["observations"]
    if [record["request"] for record in records] != [{"cmd": c, "session": session} for c in COMMANDS]:
        raise AssertionError("incomplete or reordered daemon case set")
    records = copy.deepcopy(records)
    # Rust's explicitly documented transport metadata extends Go status only.
    if rust:
        status = records[1]["response"]["data"]
        if status.pop("mode", None) != "browser" or status.pop("session", None) != session:
            raise AssertionError("invalid Rust transport metadata")

    def walk(value, key=""):
        if key == "pid":
            if value != observation["pid"]:
                raise AssertionError("reported PID does not identify the running daemon")
            return "<daemon-pid>"
        if key in ("started_at", "last_activity"):
            if not isinstance(value, str) or not value.endswith("Z"):
                raise AssertionError("daemon timestamp is not UTC")
            stamp = datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
            if not observation["begin"] - 2 <= stamp <= observation["end"] + 2:
                raise AssertionError("daemon timestamp lies outside this process run")
            return "<validated-run-time>"
        if isinstance(value, dict):
            return {k: walk(v, k) for k, v in value.items()}
        if isinstance(value, list):
            return [walk(v) for v in value]
        if isinstance(value, str):
            # Normalize only this owned process root, preserving all suffixes.
            return value.replace(observation["root"], "<owned-root>")
        return value

    return walk(records)


def compare(case: dict) -> bool:
    go = normalized(case["go"], rust=False, session=case["session"], settings=case["settings"])
    rust = normalized(case["rust"], rust=True, session=case["session"], settings=case["settings"])
    return go == rust


def controls(cases: list) -> list:
    mutations = []
    bad = copy.deepcopy(cases[0]); bad["rust"]["observations"].pop(); mutations.append(("missing-case", bad))
    bad = copy.deepcopy(cases[0]); bad["rust"]["observations"][1]["response"]["data"]["pid"] = -1
    mutations.append(("wrong-process-pid", bad))
    bad = copy.deepcopy(cases[0]); policy = bad["rust"]["observations"][1]["response"]["data"]["policy"]
    policy["fetch_ssrf_enabled"] = not policy["fetch_ssrf_enabled"]
    mutations.append(("wrong-network-policy", bad))
    results = []
    for name, bad in mutations:
        try:
            rejected = not compare(bad)
        except AssertionError:
            rejected = True
        if not rejected:
            raise AssertionError(f"negative control accepted: {name}")
        results.append({"name": name, "rejected": rejected})
    return results


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--go-source", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    source = args.go_source.resolve()
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    if revision != GO_REF or dirty:
        raise AssertionError("supplemental Go source must be immutable dcddcef0")
    cases = []
    variants = [(settings, None, False) for settings in SETTINGS]
    if os.name == "posix":
        variants.extend(({}, raw, False) for raw in (b"\xe2\x82", b"\xc0\xaf", b"\xef\xbf\xbd"))
        variants.append(({}, None, True))
    for index, (settings, raw_origin, git_origin) in enumerate(variants):
        session = f"parity{os.getpid()}-{index}"
        case = {"session": session, "settings": settings,
                "raw_origin_hex": None if raw_origin is None else raw_origin.hex(),
                "git_origin": git_origin,
                "go": observe(args.go.resolve(), settings, session, raw_origin, git_origin),
                "rust": observe(args.rust.resolve(), settings, session, raw_origin, git_origin)}
        case["matches"] = compare(case)
        cases.append(case)
    root = HERE.parents[2]
    head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    dirty = bool(subprocess.check_output(["git", "-C", str(root), "status", "--porcelain"], text=True))
    sources = subprocess.check_output(["git", "-C", str(source), "ls-files", "browse"], text=True).splitlines()
    candidate_sources = subprocess.check_output(
        ["git", "-C", str(root), "ls-files", "browse/crates", "browse/port/harness"], text=True).splitlines()
    report = {"supplemental_go_ref": GO_REF, "historical_oracle_unchanged": "652453d",
              "candidate_head": head, "candidate_dirty": dirty, "platform": platform.platform(),
              "go_binary_sha256": digest(args.go), "rust_binary_sha256": digest(args.rust),
              "go_source_sha256": {name: digest(source / name) for name in sources if (source / name).is_file()},
              "candidate_source_sha256": {name: digest(root / name) for name in candidate_sources
                                          if (root / name).is_file()},
              "total": len(cases) * len(COMMANDS), "matches": all(c["matches"] for c in cases),
              "cases": cases, "negative_controls": controls(cases)}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"{report['total']} actual daemon requests: matches={report['matches']}; 3 negative controls rejected")
    return 0 if report["matches"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
