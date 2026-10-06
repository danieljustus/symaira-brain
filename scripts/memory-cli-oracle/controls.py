#!/usr/bin/env python3
"""Require real subprocess failures for semantic stdout and exit mismatches."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust", type=Path)
    parser.add_argument("--go", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    rust = (args.rust or repo / "target/debug" / ("symbrain.exe" if os.name == "nt" else "symbrain")).resolve()
    go_cache = subprocess.run(["go", "env", "GOMODCACHE", "GOCACHE"], cwd=repo,
                              check=True, capture_output=True).stdout.decode().splitlines()
    with tempfile.TemporaryDirectory(prefix="memory-cli-control-") as temporary:
        root = Path(temporary)
        source = root / "main.go"
        source.write_bytes((repo / "scripts/memory-cli-oracle/control.go.txt").read_bytes())
        wrapper = root / ("control.exe" if os.name == "nt" else "control")
        home = root / "home"
        home.mkdir()
        env = dict(os.environ, HOME=str(home), USERPROFILE=str(home),
                   GOWORK="off", GOENV="off", CGO_ENABLED="0",
                   GOMODCACHE=go_cache[0], GOCACHE=go_cache[1])
        subprocess.run(["go", "build", "-o", str(wrapper), str(source)], cwd=root,
                       env=env, check=True, capture_output=True)
        controls = []
        for mode in ("stdout", "exit"):
            report = root / (mode + ".json")
            command = [sys.executable, str(repo / "scripts/memory-cli-oracle/replay.py"),
                       "--rust", str(wrapper), "--family", "seeded_reads", "--report", str(report)]
            if args.go:
                command.extend(("--go", str(args.go.resolve())))
            child_env = dict(env, MEMORY_CLI_CONTROL_RUST=str(rust), MEMORY_CLI_CONTROL_MODE=mode)
            child = subprocess.run(command, cwd=repo, env=child_env, capture_output=True)
            receipt = json.loads(report.read_bytes())
            assert child.returncode != 0, "negative control unexpectedly passed: " + mode
            assert receipt["cases"] == 32 and receipt["passed"] < 32, receipt
            assert all(record["database_state_unchanged"] for record in receipt["records"])
            assert b"actual Go/Rust output or database state differs" in child.stderr, child.stderr
            controls.append(dict(mode=mode, exit=child.returncode, cases=receipt["cases"],
                                 rejected_cases=receipt["cases"] - receipt["passed"],
                                 stdout=child.stdout.decode(), stderr=child.stderr.decode(),
                                 child_receipt=receipt))
        result = dict(controls=controls, operator_home_used=False,
                      wrapper_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                      wrapper_binary_sha256=hashlib.sha256(wrapper.read_bytes()).hexdigest())
        if args.report:
            args.report.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({"real_process_controls_passed": len(controls)}))


if __name__ == "__main__":
    main()
