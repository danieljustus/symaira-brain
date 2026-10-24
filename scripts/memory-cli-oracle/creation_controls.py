#!/usr/bin/env python3
"""Real-process negative controls for the store-creation family (Unix only).

Each mutant wraps the actual native CLI and then perturbs exactly one observed
aspect: created directory permission bits, stderr bytes, or the exit status.
The family replay must reject every mutant through its real comparison.
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
# Each mutant must be rejected through exactly its own observed field.
MUTANTS = {
    "directory-mode": ('/usr/bin/find "$PWD" -mindepth 1 -type d -perm 700 -exec /bin/chmod 755 {} +',
                       "tree"),
    "stderr": ("printf ' mutant' >&2", "stderr"),
    "exit": ('[ "$code" -eq 0 ] && code=7', "exit"),
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust", type=Path)
    parser.add_argument("--go", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if os.name != "posix":
        print(json.dumps({"creation_controls_skipped": "posix only"}))
        return
    repo = Path(__file__).resolve().parents[2]
    rust = (args.rust or repo / "target/debug/symbrain").resolve()
    results = []
    with tempfile.TemporaryDirectory(prefix="memory-cli-creation-control-") as temporary:
        root = Path(temporary)
        for name, (mutation, field) in MUTANTS.items():
            wrapper = root / f"mutant-{name}"
            wrapper.write_text(f'#!/bin/sh\n"{rust}" "$@"\ncode=$?\n{mutation}\nexit $code\n')
            wrapper.chmod(wrapper.stat().st_mode | stat.S_IXUSR)
            report = root / f"{name}.json"
            command = [sys.executable, str(repo / "scripts/memory-cli-oracle/replay.py"),
                       "--rust", str(wrapper), "--family", "store_creation", "--report", str(report)]
            if args.go:
                command.extend(("--go", str(args.go.resolve())))
            child = subprocess.run(command, cwd=repo, capture_output=True)
            receipt = json.loads(report.read_bytes())
            assert child.returncode != 0, "negative control unexpectedly passed: " + name
            assert receipt["cases"] > 20 and receipt["passed"] < receipt["cases"], (name, receipt["cases"], receipt["passed"])
            assert b"actual Go/Rust output or database state differs" in child.stderr, child.stderr
            rejected = [record for record in receipt["records"] if not record["match"]]
            for record in rejected:
                differing = {key for key in ("exit", "stdout", "stderr", "tree")
                             if record["go"][key] != record["rust"][key]}
                assert differing == {field}, (name, record["name"], differing)
            results.append(dict(mutant=name, exit=child.returncode, cases=receipt["cases"],
                                rejected_cases=receipt["cases"] - receipt["passed"]))
    if args.report:
        args.report.write_text(json.dumps(dict(controls=results), indent=2) + "\n")
    print(json.dumps({"creation_controls_rejected": len(results), "detail": results}))


if __name__ == "__main__":
    main()
