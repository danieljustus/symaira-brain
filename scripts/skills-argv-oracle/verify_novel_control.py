#!/usr/bin/env python3
"""Require a real changed-input mismatch, not a parent/bootstrap failure."""
import json
from pathlib import Path
import sys

report = json.loads(Path(sys.argv[1]).read_bytes())
control = sys.argv[2]
assert report["negative_control"] == control
assert report["summary"]["total"] == 1 and report["summary"]["accepted"] == 0
assert report["summary"]["readonly"] and not report["candidate_dirty"]
row = report["cases"][0]
if control == "flag-input":
    assert row["scope"] == "required-Go-native" and not row["Go_native_exact"]
    left, right = "go", "rust"
else:
    assert control == "parent-input" and row["scope"] == "actual-parent"
    assert not row["current_actual_parent_exact"]
    left, right = "rust", "parent"
assert row[left]["exit"] == row[right]["exit"] == 2
assert row[left]["stdout_hex"] == row[right]["stdout_hex"] == ""
assert row[left]["stderr_hex"] != row[right]["stderr_hex"]
assert all(report["binaries_sha256"][name] for name in ["go", "rust", "parent"])
print("Actual supplemental changed-input mismatch verified")
