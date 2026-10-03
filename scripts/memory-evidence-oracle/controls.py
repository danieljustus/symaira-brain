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
    original_bytes = fixture.read_bytes()
    original = json.loads(original_bytes)
    def encode(value):
        rendered = json.dumps(value, indent=2, ensure_ascii=False) + "\n"
        for raw, escaped in [("&", "\\u0026"), ("<", "\\u003c"), (">", "\\u003e"),
                             ("\u2028", "\\u2028"), ("\u2029", "\\u2029")]:
            rendered = rendered.replace(raw, escaped)
        return rendered
    # Preserve the Go encoder's formatting so these controls fail because of
    # the actual semantic mutation, rather than incidental reformatting.
    assert encode(original).encode() == original_bytes
    results = []
    with tempfile.TemporaryDirectory(prefix="memory-evidence-controls-") as temporary:
        root = Path(temporary)
        changed = json.loads(json.dumps(original))
        changed["alignments"][3]["status"] = "unmatched"
        (root / "changed-status.json").write_bytes(encode(changed).encode("utf-8"))
        missing_case = json.loads(json.dumps(original))
        missing_case["alignments"].pop()
        (root / "missing-case.json").write_bytes(encode(missing_case).encode("utf-8"))
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
