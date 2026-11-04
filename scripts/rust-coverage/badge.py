#!/usr/bin/env python3
"""Write coverage.json from a report.py summary and print the line total.

  badge.py SUMMARY_JSON COVERAGE_JSON
"""
import json
import sys

summary = json.load(open(sys.argv[1], encoding="utf-8"))
total = round(summary["totals"]["lines"]["percent"], 1)
crates = {name: round(counts["percent"], 1) for name, counts in sorted(summary["crates"].items())}
with open(sys.argv[2], "w", encoding="utf-8") as out:
    json.dump({"schema_version": 2, "commit_sha": summary.get("source_commit", ""), "total": total,
               "crates": crates}, out, indent=2)
    out.write("\n")
print(f"{total:.1f}")
