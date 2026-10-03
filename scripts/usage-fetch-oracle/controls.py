"""Execute missing, mutated and incomplete-oracle controls against real tests."""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    original = Path(sys.argv[1])
    output = Path(sys.argv[2])
    cases = json.loads(original.read_text())
    header = copy.deepcopy(cases)
    kimi = next(case for case in header if case["id"] == "kimi-cli/success")
    kimi["requests"][0]["headers"]["x-msh-os-version"] = "mutated-identity"
    machine = copy.deepcopy(cases)
    machine[0]["json"] = '{"mutated":true}\n'
    inputs = [
        ("missing-oracle", None, "usage", "fresh Go oracle"),
        ("mutated-identity-header", header, "usage", "exact request and all headers"),
        ("mutated-json-output", machine, "cli", "claude-api/success"),
        ("incomplete-matrix", cases[:-1], "usage", "all 19 variants"),
    ]
    results = []
    with tempfile.TemporaryDirectory(prefix="usage-620-controls-") as scratch:
        for name, payload, package, diagnostic in inputs:
            path = Path(scratch, name + ".json")
            if payload is not None:
                path.write_text(json.dumps(payload))
            env = dict(os.environ)
            env["USAGE_FETCH_ORACLE_620"] = str(path)
            env.pop("USAGE_FETCH_NATIVE_620", None)
            command = ["cargo", "test", "--locked", "-p", "symbrain-" + package]
            command += ["--lib"]
            command += ["usage_fetch_620", "--", "--nocapture"]
            result = subprocess.run(command, env=env, capture_output=True, timeout=120)
            transcript = result.stdout + result.stderr
            if result.returncode == 0 or diagnostic.encode() not in transcript:
                raise SystemExit(f"{name}: expected failure/diagnostic missing: {transcript.decode(errors='replace')}")
            results.append({
                "control": name,
                "exit_code": result.returncode,
                "expected_diagnostic": diagnostic,
                "expected_diagnostic_observed": True,
                "log_sha256": hashlib.sha256(transcript).hexdigest(),
            })
    receipt = {
        "candidate_source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "oracle_sha256": hashlib.sha256(original.read_bytes()).hexdigest(),
        "all_controls_failed_as_expected": True,
        "results": results,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(receipt, indent=2) + "\n")
    print("usage #620: all four real negative controls failed with their expected diagnostics")


if __name__ == "__main__":
    main()
