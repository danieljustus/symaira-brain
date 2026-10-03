#!/usr/bin/env python3
"""Require three actual failing replay processes with the intended diagnostics."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    fixture = repo / "rust/symbrain-memory/tests/fixtures/memory_evidence_go_v017.json"
    original = json.loads(fixture.read_bytes())
    results = []
    with tempfile.TemporaryDirectory(prefix="memory-evidence-controls-") as temporary:
        root = Path(temporary)
        changed = json.loads(json.dumps(original))
        changed["alignments"][3]["status"] = "unmatched"
        (root / "changed-status.json").write_text(json.dumps(changed))
        missing_case = json.loads(json.dumps(original))
        missing_case["alignments"].pop()
        (root / "missing-case.json").write_text(json.dumps(missing_case))
        for name, reason in [("missing-fixture", "FileNotFoundError"),
                             ("changed-status", "actual Go output differs"),
                             ("missing-case", "actual Go output differs")]:
            completed = subprocess.run(
                [sys.executable, str(repo / "scripts/memory-evidence-oracle/replay.py"),
                 "--fixture", str(root / (name + ".json"))], cwd=repo,
                capture_output=True, text=True, timeout=180,
            )
            log = completed.stdout + completed.stderr
            if completed.returncode == 0 or reason not in log:
                raise RuntimeError(f"control {name} did not fail for {reason}: {log}")
            args.report.with_name("memory-evidence-control-" + name + ".log").write_text(log)
            results.append({"case": name, "exit_code": completed.returncode,
                            "required_reason": reason, "observed_reason": True})
    args.report.write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
