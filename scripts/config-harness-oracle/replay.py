#!/usr/bin/env python3
"""Replay `config` and `harness` argv grammar against the immutable Go CLI.

Every Rust process runs with an absent Go fallback, so a shape that still
routes to Go cannot match. Output, exit code and (for mutating cases) the full
fixture filesystem with modes must equal the Go oracle's.
"""
from __future__ import annotations

from dataclasses import replace
import hashlib
import importlib.util
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ORACLE_REF = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("cfgh_legacy", ROOT / "scripts/rust-differential.py")
legacy = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = legacy
spec.loader.exec_module(legacy)

CONFIG_SHAPES = [
    ("-h",), ("--h",), ("-help",), ("--help",), ("-help=1",), ("-x",), ("--x",), ("---x",),
    ("----x",), ("--=x",), ("-=x",), ("-x=1",), ("--x=1",), ("-",), ("--", "-x"),
    ("--", "--x"), ("--", "-"), ("--", "frobnicate"), ("-x", "path"), ("-h", "path"),
    ("--",), ("--", "path"), ("--", "get", "-x"), ("-json", "path"), ("--json", "path"),
    ("--output", "json", "path"), ("path", "-h"), ("frobnicate", "-x"),
    (b"\xff",), (b"-\xff",), (b"caf\xc3\xa9\xff",), ("--", b"\xff"),
]
HARNESS_SHAPES = [
    ("-h",), ("--help",), ("help",), ("-help",), ("frobnicate",), ("LIST",), ("--json",),
    ("--json", "list"), ("--output", "table", "list"), ("--output=json", "list"),
    (b"\xff",), (b"list\xff",),
]
FLAG_SHAPES = [
    ("-h",), ("--h",), ("-help",), ("--help",), ("-help=1",), ("--bogus",), ("-bogus=1",),
    ("---x",), ("--=x",), ("-=x",), ("-",), ("--",), ("--", "x"), ("--", "-x"), ("x",),
    ("x", "-bogus"), ("-bogus", "x"), ("-project",), ("--project=",), ("-project", ""),
    ("--project", "/nonexistent-cfgh-project"), ("-project=/nonexistent-cfgh-project",),
    ("--project", "PROJECT"), ("-project", "a", "-project", "b"), ("-project", "-h"),
    ("-project", "--"), ("-project", "--", "x"), ("--json",), ("--output=json",),
    (b"\xff",), (b"-\xff",),
]
HEALTH_ONLY = [
    ("-harness",), ("--harness",), ("--harness=",), ("--harness", ""), ("--harness=nope",),
    ("--harness", "claude"), ("--harness", "claude", "--project", "PROJECT"),
    ("-harness", "-project"), ("--harness", b"\xff"), ("-harness", "cursor", "-x"),
]


def new_cases():
    cases = []
    for index, argv in enumerate(CONFIG_SHAPES):
        cases.append(legacy.Case(f"cfgh-config-{index}", ("config", *argv),
                                 setup=legacy.setup_xdg_config, mutating=True))
    for index, argv in enumerate(HARNESS_SHAPES):
        cases.append(legacy.Case(f"cfgh-harness-{index}", ("harness", *argv)))
    for verb in ("list", "health"):
        shapes = FLAG_SHAPES + (HEALTH_ONLY if verb == "health" else [])
        for index, argv in enumerate(shapes):
            cases.append(legacy.Case(f"cfgh-{verb}-{index}", ("harness", verb, *argv)))
    return cases


# These legacy probes launch SYMBRAIN_GO_BINARY as their fake MCP server, so
# they cannot run with the fallback path removed; rust-differential.py still
# replays them.
NEEDS_GO_BINARY = ("harness_health_initialize", "harness_health_initialize_json")


def selected_cases():
    existing = [case for case in legacy.CASES
                if case.name.startswith(("config_", "harness_")) and case.name not in NEEDS_GO_BINARY]
    return existing + new_cases()


def skipped(case):
    if os.name != "nt":
        return None
    if case.posix_only:
        return "POSIX-only filesystem/process behavior"
    if any(isinstance(arg, bytes) for arg in case.argv):
        return "raw-byte argv is not representable by Windows process APIs"
    return None


def text_field(name, data):
    try:
        return {name: data.decode("utf-8")}
    except UnicodeDecodeError:
        return {f"{name}_hex": data.hex()}


def fixture_entry(case, observation):
    """Root-independent Go bytes for a case, or None when paths appear."""
    out, err = observation["stdout"], observation["stderr"]
    if b"<root>" in out or b"<root>" in err:
        return None
    return {
        "name": case.name,
        "argv_hex": [(a if isinstance(a, bytes) else a.encode()).hex() for a in case.argv],
        "raw_bytes": any(isinstance(a, bytes) for a in case.argv),
        "exit": observation["exit"],
        **text_field("stdout", out),
        **text_field("stderr", err),
    }


def main():
    args = sys.argv[1:]
    fixture_path = None
    if "--record-fixture" in args:
        at = args.index("--record-fixture")
        fixture_path = args[at + 1]
        del args[at:at + 2]
    if len(args) != 3:
        raise SystemExit("usage: replay.py GO_BINARY RUST_BINARY REPORT_JSON [--record-fixture PATH]")
    go, rust, report = args
    cases = selected_cases()
    assert len({case.name for case in cases}) == len(cases)
    assert sum(case.name.startswith("cfgh-") for case in cases) >= 100, "new corpus shrank"
    legacy.CASES = tuple(replace(case, env_overrides={
        **(case.env_overrides or {}), "SYMBRAIN_GO_BINARY": str(ROOT / "absent-go-fallback")
    }) for case in cases)
    executed = [case for case in cases if not skipped(case)]
    skip_report = {case.name: skipped(case) for case in cases if skipped(case)}
    observations = []
    actual_run = legacy.run

    def recorded_run(binary, argv, env, stdin=None, pty=False):
        completed = actual_run(binary, argv, env, stdin, pty)
        if Path(binary).resolve() in (Path(go).resolve(), Path(rust).resolve()):
            fixture_root = Path(env["PROJECT"]).parent
            observations.append({
                "binary": "go" if Path(binary).resolve() == Path(go).resolve() else "rust",
                "exit": completed.returncode,
                "stdout": legacy.normalize_fixture_root(completed.stdout, fixture_root),
                "stderr": legacy.normalize_fixture_root(completed.stderr, fixture_root),
            })
        return completed

    legacy.run = recorded_run
    sys.argv = [str(ROOT / "scripts/rust-differential.py"), go, rust]
    code = legacy.main()
    assert len(observations) == 2 * len(executed), "incomplete process replay"
    pairs = list(zip(observations[0::2], observations[1::2]))
    assert all(a["binary"] == "go" and b["binary"] == "rust" for a, b in pairs)
    # Legacy cases may carry documented per-case normalizations (HAR-007), so
    # the independent raw-byte cross-check covers the new corpus only.
    mismatched = [case.name for case, (a, b) in zip(executed, pairs)
                  if case.name.startswith("cfgh-") and (a["exit"], a["stdout"], a["stderr"]) != (b["exit"], b["stdout"], b["stderr"])]
    assert not mismatched or code, "byte mismatch not reflected in the harness verdict"
    data = {
        "go_oracle_ref": ORACLE_REF,
        "candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
        "runtime": platform.platform(), "selected": len(cases), "executed": len(executed),
        "skipped": skip_report, "exit": code, "matches": code == 0, "mismatched": mismatched,
        "new_cases": sum(case.name.startswith("cfgh-") for case in executed),
        "legacy_cases": sum(not case.name.startswith("cfgh-") for case in executed),
        "observations": [{"binary": o["binary"], "exit": o["exit"],
                          "stdout_sha256": hashlib.sha256(o["stdout"]).hexdigest(),
                          "stderr_sha256": hashlib.sha256(o["stderr"]).hexdigest()}
                         for o in observations],
        "cases": [case.name for case in executed],
        "go_binary_sha256": hashlib.sha256(Path(go).read_bytes()).hexdigest(),
        "rust_binary_sha256": hashlib.sha256(Path(rust).read_bytes()).hexdigest(),
        "comparison": "stdout/stderr/exit for every case; full fixture filesystem and modes for mutating cases; only the fixture root is normalized",
        "fallback": "every Rust process has an absent SYMBRAIN_GO_BINARY; a shape routed to Go cannot match",
    }
    names = subprocess.check_output(
        ["git", "ls-tree", "-r", "--name-only", ORACLE_REF, "--", "cmd/symbrain", "internal/harness",
         "internal/config", "go.mod", "go.sum"], cwd=ROOT, text=True).splitlines()
    data["go_source_sha256"] = {name: hashlib.sha256(subprocess.check_output(
        ["git", "show", f"{ORACLE_REF}:{name}"], cwd=ROOT)).hexdigest()
        for name in names if name.endswith(".go") or name in ("go.mod", "go.sum")}
    Path(report).write_text(json.dumps(data, indent=2) + "\n")
    if fixture_path and code == 0:
        entries = [entry for case, (go_obs, _) in zip(executed, pairs)
                   if case.name.startswith("cfgh-") and (entry := fixture_entry(case, go_obs))]
        Path(fixture_path).write_text(json.dumps(
            {"go_oracle_ref": ORACLE_REF, "cases": entries}, indent=1, sort_keys=True) + "\n")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
