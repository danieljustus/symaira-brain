#!/usr/bin/env python3
"""Reject bootstrap errors masquerading as a successful negative control."""
import json
from pathlib import Path
import sys

report = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
assert report["negative_control"] == sys.argv[2]
assert report["total"] == 1 and report["matched"] == 0
assert report["readonly"] and not report["candidate_dirty"]
row = report["results"][0]
assert row["id"] == "actual-input-control" and not row["matched"]
assert row["go"]["exit_code"] == row["rust"]["exit_code"] == 2
assert row["go"]["stdout_hex"] == row["rust"]["stdout_hex"] == ""
assert row["go"]["stderr_hex"] != row["rust"]["stderr_hex"]
assert all(report["binaries_sha256"][name] for name in ["go", "rust"])
print("Actual mismatching input control retained; bootstrap errors excluded")
