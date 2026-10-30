#!/usr/bin/env python3
"""Real-process negative controls for the `sync_preflight` family (Unix only).

Each mutant wraps the actual native CLI and perturbs exactly one observed
aspect of a refusal: stdout bytes, stderr bytes or a failing exit status. The
family replay must reject every mutant, and every rejected refusal record must
differ from the immutable Go process through exactly the mutated field. Gate
records only assert that real synchronisation stays on Go, so they take no part
in the field attribution.
"""
import argparse
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile

# Replay children run with an empty PATH, so mutants use absolute tools only.
MUTANTS = {
    "stdout": ("printf 'mutant'", "stdout"),
    "stderr": ("printf ' mutant' >&2", "stderr"),
    "exit": ('[ "$code" -ne 0 ] && [ "$code" -ne 1 ] && code=7', "exit"),
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust", type=Path)
    parser.add_argument("--go", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if os.name != "posix":
        print(json.dumps({"sync_controls_skipped": "posix only"}))
        return
    repo = Path(__file__).resolve().parents[2]
    rust = (args.rust or repo / "target/debug/symbrain").resolve()
    results = []
    with tempfile.TemporaryDirectory(prefix="memory-cli-sync-control-") as temporary:
        root = Path(temporary)
        for name, (mutation, field) in MUTANTS.items():
            wrapper = root / f"mutant-{name}"
            wrapper.write_text(f'#!/bin/sh\n"{rust}" "$@"\ncode=$?\n{mutation}\nexit $code\n')
            wrapper.chmod(wrapper.stat().st_mode | stat.S_IXUSR)
            report = root / f"{name}.json"
            command = [sys.executable, str(repo / "scripts/memory-cli-oracle/replay.py"),
                       "--rust", str(wrapper), "--family", "sync_preflight", "--report", str(report)]
            if args.go:
                command.extend(("--go", str(args.go.resolve())))
            child = subprocess.run(command, cwd=repo, capture_output=True)
            receipt = json.loads(report.read_bytes())
            assert child.returncode != 0, "negative control unexpectedly passed: " + name
            assert receipt["cases"] > 100 and receipt["passed"] < receipt["cases"], (name, receipt["cases"], receipt["passed"])
            assert b"actual Go/Rust output or database state differs" in child.stderr, child.stderr
            rejected = [record for record in receipt["records"] if not record["match"] and record["kind"] == "refusal"]
            assert rejected, name
            for record in rejected:
                differing = {key for key in ("exit", "stdout", "stderr", "tree")
                             if record["go"][key] != record["rust"][key]}
                assert differing == {field}, (name, record["name"], differing)
            results.append(dict(mutant=name, exit=child.returncode, cases=receipt["cases"],
                                rejected_cases=receipt["cases"] - receipt["passed"]))
    if args.report:
        args.report.write_text(json.dumps(dict(controls=results), indent=2) + "\n")
    print(json.dumps({"sync_controls_rejected": len(results), "detail": results}))


if __name__ == "__main__":
    main()
