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
from raw_path_admission import probe, unavailable_is_proven, validate_accounting, accounting_controls

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


def private_temporary_parent() -> str | None:
    parent = harness.temporary_parent(os.environ)
    if sys.platform == "darwin" and os.environ.get("CI"):
        # Darwin sockaddr_un is bounded; runner TMPDIR is often deeply nested.
        # Keep this owned private HOME short without changing daemon endpoints.
        parent = "/tmp"
    return parent


def absent_keychain_path(root: Path) -> str:
    """Go treats a missing security executable as failure, not key absence."""
    if sys.platform != "darwin":
        return ""
    owned_bin = root / "provider-bin"
    owned_bin.mkdir(mode=0o700, exist_ok=True)
    security = owned_bin / "security"
    security.write_text("#!/bin/sh\nexit 44\n")
    security.chmod(0o700)
    return str(owned_bin)


def observe(binary: Path, settings: dict[str, str], session: str,
            raw_origin: bytes | None = None, git_origin: bool = False) -> dict:
    with tempfile.TemporaryDirectory(prefix="bd-", dir=private_temporary_parent()) as temporary:
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
                   XDG_RUNTIME_DIR=str(runtime), PATH=absent_keychain_path(root), SYMBROWSE_NO_AUTOSTART="1",
                   TMPDIR=str(temporary), TMP=str(temporary), TEMP=str(temporary))
        env.update(settings)
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
            env["PATH"] = str(git_bin) + os.pathsep + absent_keychain_path(root)
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
    report = {"status": "started", "matches": False, "total": 0,
              "cases": [], "unavailable_cases": [], "requested_cases": []}
    save_report(args, report)
    try:
        return run_gate(args, report)
    except BaseException as error:
        report.update(status="failed", failure={"type": type(error).__name__, "message": str(error)})
        save_report(args, report)
        raise


def save_report(args, report):
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")


def run_gate(args, report):
    source = args.go_source.resolve()
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    if revision != GO_REF or dirty:
        raise AssertionError("supplemental Go source must be immutable dcddcef0")
    root = HERE.parents[2]
    head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    dirty = bool(subprocess.check_output(["git", "-C", str(root), "status", "--porcelain"], text=True))
    sources = subprocess.check_output(["git", "-C", str(source), "ls-files", "browse"], text=True).splitlines()
    candidate_sources = subprocess.check_output(
        ["git", "-C", str(root), "ls-files", "browse/crates", "browse/port/harness"], text=True).splitlines()
    report.update({"supplemental_go_ref": GO_REF, "historical_oracle_unchanged": "652453d",
              "candidate_head": head, "candidate_dirty": dirty, "platform": platform.platform(),
              "go_binary_sha256": digest(args.go), "rust_binary_sha256": digest(args.rust),
              "go_source_sha256": {name: digest(source / name) for name in sources if (source / name).is_file()},
              "candidate_source_sha256": {name: digest(root / name) for name in candidate_sources
                                          if (root / name).is_file()}})
    save_report(args, report)
    report.update(platform_system=sys.platform, cases=[], unavailable_cases=[], requested_cases=[])
    variants = [(settings, None, False) for settings in SETTINGS]
    if os.name == "posix":
        variants.extend(({}, raw, False) for raw in (b"\xe2\x82", b"\xc0\xaf", b"\xef\xbf\xbd"))
        variants.append(({}, None, True))
    for index, (settings, raw_origin, git_origin) in enumerate(variants):
        report["requested_cases"].append({"id": f"process-{index}", "session": f"parity{os.getpid()}-{index}",
                "settings": settings, "raw_origin_hex": None if raw_origin is None else raw_origin.hex(),
                "git_origin": git_origin})
    save_report(args, report)
    for requested in report["requested_cases"]:
        settings, git_origin, session = requested["settings"], requested["git_origin"], requested["session"]
        raw_origin = None if requested["raw_origin_hex"] is None else bytes.fromhex(requested["raw_origin_hex"])
        case = dict(requested)
        # The live record precedes probe/child operations and survives failures.
        report["cases"].append(case)
        save_report(args, report)
        if raw_origin is not None:
            case["kernel_probe"] = {}
            probe(raw_origin, private_temporary_parent(), case["kernel_probe"])
            save_report(args, report)
            if not case["kernel_probe"]["admitted"]:
                if not unavailable_is_proven(case["kernel_probe"], raw_origin):
                    raise AssertionError("raw filename exclusion lacks exact owned Darwin EILSEQ92 proof")
                report["cases"].remove(case)
                case.update(child_executed=False, parity=False)
                report["unavailable_cases"].append(case)
                save_report(args, report)
                continue
        for owner, binary in (("go", args.go), ("rust", args.rust)):
            case[owner] = observe(binary.resolve(), settings, session, raw_origin, git_origin)
            save_report(args, report)
        case["matches"] = compare(case)
        save_report(args, report)
    cases = report["cases"]
    report.update({"total": len(cases) * len(COMMANDS), "matches": all(c["matches"] for c in cases),
              "cases": cases, "negative_controls": controls(cases), "status": "complete"})
    report.update(matches_executed=report["matches"], platform_admitted_complete=validate_accounting(report),
                  original_requested_domain_complete=not report["unavailable_cases"])
    report["domain_accounting_controls"] = accounting_controls(report)
    save_report(args, report)
    print(f"{report['total']} actual daemon requests: matches_executed={report['matches_executed']}; "
          f"platform_admitted_complete={report['platform_admitted_complete']}; "
          f"unavailable_cases={len(report['unavailable_cases'])}; 3 original negative controls rejected")
    return 0 if report["matches"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
