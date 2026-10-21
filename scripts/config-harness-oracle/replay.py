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
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ORACLE_REF = "0541e64a36d83bac8e5e4f8c4136033c03db83ae"
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("cfgh_legacy", ROOT / "scripts/rust-differential.py")
legacy = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = legacy
spec.loader.exec_module(legacy)
from harness_health_contract import SEEDED_HEALTH_PREFIX  # noqa: E402  (needs scripts/ on sys.path)

# Seeded cases bound every probe: a hang must fail instead of waiting for the
# shared harness timeout. Rust itself caps a probe at 5 s.
SEEDED_PROBE_BUDGET_S = 8.0

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

# Seeded harness configs (#769 review F1/F2). Every server fails fast and
# offline: a stdio command that is not on PATH and an HTTP endpoint on a closed
# loopback port (Go never probes HTTP, Rust gets connection refused).
SEEDED_GLOBAL = {"mcpServers": {
    "a": {"command": "symaira-missing-mcp-fixture", "args": ["x"]},
    "b": {"type": "http", "url": "http://127.0.0.1:1/mcp"},
}}
SEEDED_PROJECT = {"mcpServers": {"p": {"command": "symaira-missing-mcp-fixture"}}}
SEEDED_HEALTH = [
    (), ("-harness=",), ("-harness", ""), ("--harness", ""), ("-harness", "claude"),
    ("-harness", "nope"), ("-harness", "claude", "-harness="), ("-harness=", "-harness", "claude"),
    ("-harness", "cursor"), ("-project", "PROJECT"), ("-project", "PROJECT", "-harness="),
    ("-project", "PROJECT", "-harness", ""), ("-project", "PROJECT", "-harness", "claude"),
    ("-project", "PROJECT", "-harness", "nope"), ("-project", ".", "-harness="),
    ("-project", "./"), ("-project", "./x/../", "-harness", "claude"),
    ("--json",), ("--json", "-harness="), ("--json", "-harness", "nope"),
    ("--json", "-project", "PROJECT", "-harness="), ("--json", "-project", ".", "-harness", ""),
]
SEEDED_LIST = [
    (), ("-project=",), ("-project", ""), ("-project", "PROJECT"), ("-project", "."),
    ("-project", "./"), ("-project", "./x/../"), ("--json",), ("--json", "-project", "PROJECT"),
    ("--json", "-project", "."), ("--json", "-project", "./"), ("--json", "-project", "./x/../"),
]


def setup_seeded_harness(root, env):
    home = Path(env["HOME"])
    (home / ".claude.json").write_text(json.dumps(SEEDED_GLOBAL), encoding="utf-8")
    (root / "project" / ".mcp.json").write_text(json.dumps(SEEDED_PROJECT), encoding="utf-8")


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
    for index, argv in enumerate(SEEDED_HEALTH):
        cases.append(legacy.Case(f"{SEEDED_HEALTH_PREFIX}{index}", ("harness", "health", *argv),
                                 setup=setup_seeded_harness))
    for index, argv in enumerate(SEEDED_LIST):
        cases.append(legacy.Case(f"cfgh-seeded-list-{index}", ("harness", "list", *argv),
                                 setup=setup_seeded_harness))
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
    # Seeded cases depend on files the cargo fixture test does not create.
    if case.name.startswith("cfgh-seeded-") or b"<root>" in out or b"<root>" in err:
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
    # Re-exec through the external-storage wrapper here, not inside
    # legacy.main(): its own re-exec would restart rust-differential.py and
    # replay the legacy suite instead of this corpus, then exit 0 with no report.
    legacy.ensure_external_environment(__file__)
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
        started = time.perf_counter()
        completed = actual_run(binary, argv, env, stdin, pty)
        elapsed = time.perf_counter() - started
        if Path(binary).resolve() in (Path(go).resolve(), Path(rust).resolve()):
            fixture_root = Path(env["PROJECT"]).parent
            observations.append({
                "binary": "go" if Path(binary).resolve() == Path(go).resolve() else "rust",
                "exit": completed.returncode,
                "stdout": legacy.normalize_fixture_root(completed.stdout, fixture_root),
                "stderr": legacy.normalize_fixture_root(completed.stderr, fixture_root),
                "elapsed_s": elapsed,
            })
        return completed

    legacy.run = recorded_run
    sys.argv = [str(ROOT / "scripts/rust-differential.py"), go, rust]
    code = legacy.main()
    assert len(observations) == 2 * len(executed), "incomplete process replay"
    pairs = list(zip(observations[0::2], observations[1::2]))
    assert all(a["binary"] == "go" and b["binary"] == "rust" for a, b in pairs)
    # Legacy cases and the seeded health cases carry documented HAR-007
    # normalizations, so the independent raw-byte cross-check covers the rest
    # of the new corpus.
    mismatched = [case.name for case, (a, b) in zip(executed, pairs)
                  if case.name.startswith("cfgh-") and not case.name.startswith(SEEDED_HEALTH_PREFIX)
                  and (a["exit"], a["stdout"], a["stderr"]) != (b["exit"], b["stdout"], b["stderr"])]
    assert not mismatched or code, "byte mismatch not reflected in the harness verdict"
    seeded_elapsed = [o["elapsed_s"] for case, pair in zip(executed, pairs) for o in pair
                      if case.name.startswith("cfgh-seeded-")]
    assert seeded_elapsed, "seeded harness cases did not run"
    assert max(seeded_elapsed) < SEEDED_PROBE_BUDGET_S, f"seeded probe took {max(seeded_elapsed):.2f}s"
    data = {
        "go_oracle_ref": ORACLE_REF,
        "candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
        "runtime": platform.platform(), "selected": len(cases), "executed": len(executed),
        "skipped": skip_report, "exit": code, "matches": code == 0, "mismatched": mismatched,
        "new_cases": sum(case.name.startswith("cfgh-") for case in executed),
        "seeded_cases": sum(case.name.startswith("cfgh-seeded-") for case in executed),
        "max_seeded_elapsed_s": round(max(seeded_elapsed), 3),
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
