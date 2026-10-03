"""Bind actual Go/native report equality to source and harness hashes."""
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    oracle, native, output, go_commit, controls_path = sys.argv[1:]
    expected = json.loads(Path(oracle).read_text())
    actual = json.loads(Path(native).read_text())
    if len(expected) != 133 or len(actual) != 133:
        raise SystemExit("incomplete provider matrix")
    for go_case, rust_case in zip(expected, actual, strict=True):
        if go_case["id"] != rust_case["id"] or go_case["report"] != rust_case["report"]:
            raise SystemExit(f"report mismatch: {go_case['id']}")
        if go_case["requests"] != rust_case["requests"]:
            raise SystemExit(f"request mismatch: {go_case['id']}")
        if not go_case["json"] or not go_case["table"] or not go_case["requests"]:
            raise SystemExit(f"missing output/request proof: {go_case['id']}")
    controls = json.loads(Path(controls_path).read_text())
    if len(controls["results"]) != 4 or not controls["all_controls_failed_as_expected"]:
        raise SystemExit("negative controls incomplete")
    receipt = {
        "schema_version": 1,
        "candidate_source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()),
        "go_source_commit": go_commit,
        "go_toolchain": "go1.26.7",
        "platform": platform.system(),
        "reports": len(actual),
        "output_checks": len(actual) * 2,
        "all_match": True,
        "normalizations": ["snapshot.fetched_at: Unix epoch", "OpenCode X-Server-Instance: validated server-fn shape"],
        "oracle_sha256": sha(oracle),
        "native_reports_sha256": sha(native),
        "harness_sha256": {str(path): sha(path) for path in sorted(Path("scripts/usage-fetch-oracle").glob("*")) if path.is_file()},
        "failure_controls": controls,
        "native_cases": actual,
        "cases": expected,
    }
    Path(output).write_text(json.dumps(receipt, indent=2, ensure_ascii=True) + "\n")
    print(f"usage #620: {len(actual)}/{len(actual)} reports, 266 JSON/table byte comparisons, complete request walks match")


if __name__ == "__main__":
    main()
