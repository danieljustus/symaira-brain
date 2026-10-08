#!/usr/bin/env python3
"""Compare complete Memory MCP catalogs with real frozen-Go/native processes."""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("mcp_frames", REPO / "scripts/mcp-differential.py")
protocol = importlib.util.module_from_spec(spec)
spec.loader.exec_module(protocol)
ORACLE = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
ALL = ["entity_list", "entity_relate", "entity_resolve", "graph_neighbors",
       "memory_candidates", "memory_get", "memory_list", "memory_promote",
       "memory_reject", "memory_search", "memory_set", "query_log",
       "activity_get", "activity_search", "activity_status"]
CASES = [
    ("read-only", 'enabled=true\nmode="read_only"'),
    ("read-write", 'enabled=true\nmode="read_write"'),
    ("explicit-all", 'enabled=true\nmode="read_write"\ntools_allow=' + json.dumps(ALL)),
    ("explicit-one", 'enabled=true\nmode="read_write"\ntools_allow=["memory_get"]'),
    ("deny-writes", 'enabled=true\nmode="read_write"\ntools_deny=["memory_set","entity_relate"]'),
    ("disabled", "enabled=false"),
    ("allow-order", 'enabled=true\nmode="read_write"\ntools_allow=["entity_relate","memory_search","memory_get"]'),
    ("activity-only", 'enabled=true\nmode="read_only"\ntools_allow=["activity_status","activity_get","activity_search"]'),
]
PROFILE = '''[profile]
name="mcp-oracle"
[servers.vault]
enabled=false
[servers.memory]
{memory}
[servers.skills]
enabled=false
[servers.usage]
enabled=false
[audit]
enabled=false
'''
REQUESTS = (
    protocol.request(1, "initialize", {"protocolVersion": "2024-11-05",
                                     "capabilities": {},
                                     "clientInfo": {"name": "owned-catalog", "version": "1"}}),
    protocol.request(None, "notifications/initialized"),
    protocol.request(2, "tools/list"),
)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def wire(binary, root, profile, framing, alias, mutant=None):
    root.mkdir(parents=True)
    env = protocol.environment(root)
    env.update(PATH="", SYMBRAIN_GO_BINARY=str(root / "missing-go"),
               SYMBRAIN_GO_FALLBACK_BIN=str(root / "missing-go"))
    command = [str(binary), alias, "--profile-file", str(profile)]
    if mutant:
        command = [sys.executable, str(Path(__file__).resolve()), "--mutator", mutant, *command]
    result = subprocess.run(command, input=protocol.encode_requests(REQUESTS, framing),
                            capture_output=True, cwd=root, env=env, timeout=15)
    return {"exit": result.returncode,
            "stdout": base64.b64encode(result.stdout).decode(),
            "stderr": base64.b64encode(result.stderr).decode()}


def mutate(kind, command):
    data = sys.stdin.buffer.read()
    child = subprocess.run(command, input=data, capture_output=True, timeout=15)
    framing = "content-length" if data.startswith(b"Content-Length:") else "line"
    frames = protocol.frames(child.stdout, framing)
    values = [value for _, value in frames]
    tools = next(value["result"]["tools"] for value in values if value.get("id") == 2)
    target = next(tool for tool in tools if tool["name"] == "memory_get")
    if kind == "title":
        target["annotations"]["title"] = "wrong title"
    elif kind == "hint":
        target["annotations"]["idempotentHint"] = True
    elif kind == "schema":
        target["inputSchema"]["required"] = []
    elif kind == "order":
        tools.reverse()
    elif kind == "exposure":
        tools.append({"name": "memory_reject", "inputSchema": {"type": "object"}})
    else:
        raise AssertionError(kind)
    sys.stdout.buffer.write(protocol.encode_requests(tuple(values), framing))
    sys.stderr.buffer.write(child.stderr)
    return child.returncode


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    go, rust = args.go.resolve(), args.rust.resolve()
    records, controls = [], []
    with tempfile.TemporaryDirectory(prefix="memory-mcp-catalog-") as temporary:
        root = Path(temporary).resolve()
        for name, memory in CASES:
            profile = root / (name + ".toml")
            profile.write_text(PROFILE.format(memory=memory), encoding="utf-8")
            for framing in ("line", "content-length"):
                for alias in ("mcp", "serve"):
                    case = f"{name}-{framing}-{alias}"
                    first = wire(go, root / (case + "-go"), profile, framing, alias)
                    second = wire(rust, root / (case + "-rust"), profile, framing, alias)
                    decoded = protocol.frames(base64.b64decode(first["stdout"]), framing)
                    tools = next(value["result"]["tools"] for _, value in decoded if value.get("id") == 2)
                    expected_stderr = protocol.DEPRECATED_SERVE if alias == "serve" else b""
                    valid = (first["exit"] == 0 and
                             base64.b64decode(first["stderr"]) == expected_stderr and
                             len(decoded) == 2 and [x[1]["id"] for x in decoded] == [1, 2] and
                             not any(tool["name"] in ("memory_candidates", "memory_promote",
                                                       "memory_reject", "query_log") for tool in tools))
                    records.append({"case": case, "profile": profile.read_text(),
                                    "match": valid and first == second,
                                    "go": first, "rust": second,
                                    "names": [tool["name"] for tool in tools]})
        profile = root / "explicit-all.toml"
        expected = next(record["go"] for record in records
                        if record["case"] == "explicit-all-line-mcp")
        for kind in ("title", "hint", "schema", "order", "exposure"):
            result = wire(rust, root / ("control-" + kind), profile, "line", "mcp", mutant=kind)
            controls.append({"kind": kind, "rejected": result != expected,
                             "actual_child_exit": result["exit"], "go": expected, "mutant": result})
    tracked = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard",
        "rust/symbrain-gateway", "rust/symbrain-mcp", "rust/symbrain-policy", "rust/symbrain-cli",
        "rust/symbrain-memory", "rust/symbrain-activity", "scripts/memory-mcp-oracle",
        "scripts/mcp-differential.py", ".github/workflows/memory-mcp-native.yml", "Cargo.toml", "Cargo.lock"],
        cwd=REPO, text=True).splitlines()
    source = {name: digest((REPO / name).read_bytes()) for name in tracked if (REPO / name).is_file()}
    report = {"frozen_go": ORACLE,
              "candidate_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
              "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=REPO).strip()),
              "go_binary_sha256": digest(go.read_bytes()), "rust_binary_sha256": digest(rust.read_bytes()),
              "candidate_source_sha256": source, "cases": len(records),
              "passed": sum(record["match"] for record in records),
              "controls": controls, "records": records,
              "scope": "complete catalog/initialize stream, exit, diagnostics and exposure; not filesystem or tools/call parity"}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"cases": report["cases"], "passed": report["passed"],
                      "controls_rejected": sum(control["rejected"] for control in controls),
                      "source_files": len(source)}))
    assert report["passed"] == report["cases"], "complete Go/native catalog stream differs"
    assert all(control["rejected"] and control["actual_child_exit"] == 0 for control in controls)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--mutator":
        raise SystemExit(mutate(sys.argv[2], sys.argv[3:]))
    main()
