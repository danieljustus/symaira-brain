#!/usr/bin/env python3
"""Recompute the #773 fetch p95 value gate from raw benchmark samples.

Go was removed (#1003), so the recorded Go p95 (5.894 ms) times 1.10 is a fixed
ceiling: 6.4834 ms.  Reports come from ``port/bench/run.py --rust`` and keep the
pinned ``rust016-static-fetch-html-v1`` workload.  p95 is recomputed here from the
raw samples (nearest rank), never copied from the report's own summary field.
"""
from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from pathlib import Path

THRESHOLD_NS = 6_483_400  # 6.4834 ms = recorded Go p95 5.894 ms x 1.10
FIXTURE = "rust016-static-fetch-html-v1"


def p95_ns(samples: list[int]) -> int:
    ordered = sorted(samples)
    return ordered[max(0, math.ceil(0.95 * len(ordered)) - 1)]


def check(path: Path, minimum_samples: int) -> dict[str, object]:
    report = json.loads(path.read_text())
    fetch = report["binaries"]["rust"]["fetch"]
    problems: list[str] = []
    if report.get("workload_fixture") != FIXTURE:
        problems.append(f"workload fixture is {report.get('workload_fixture')!r}, not {FIXTURE}")
    if fetch.get("status") != "pass":
        problems.append(f"fetch workload status {fetch.get('status')!r}")
    control = fetch.get("semantic_contract", {}).get("negative_control", {})
    if control.get("rejected") is not True:
        problems.append("wrong-but-fast negative control was not rejected")
    raw = [item["duration_ns"] for item in fetch.get("raw_samples", [])]
    if len(raw) < minimum_samples or any(not isinstance(value, int) or value <= 0 for value in raw):
        problems.append(f"need >= {minimum_samples} positive integer samples, found {len(raw)}")
    if fetch.get("statuses") != ["pass"] * len(raw):
        problems.append("not every sample passed the response-semantics check")
    p95 = p95_ns(raw) if raw else 0
    if raw and p95 != fetch.get("p95_duration_ns"):
        problems.append("report p95 field disagrees with the raw samples")
    return {
        "report": path.name,
        "samples": len(raw),
        "p95_ns": p95,
        "p95_ms": round(p95 / 1e6, 4),
        "median_ms": round(statistics.median(raw) / 1e6, 4) if raw else None,
        "max_ms": round(max(raw) / 1e6, 4) if raw else None,
        "under_threshold": bool(raw) and p95 <= THRESHOLD_NS,
        "binary_sha256": report["binaries"]["rust"]["identity"]["sha256"],
        "problems": problems,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reports", nargs="+", type=Path)
    parser.add_argument("--minimum-samples", type=int, default=30)
    parser.add_argument("--summary", type=Path, help="write the recomputed summary JSON here")
    args = parser.parse_args(argv)
    rows = [check(path, args.minimum_samples) for path in args.reports]
    passed = all(row["under_threshold"] and not row["problems"] for row in rows)
    summary = {"threshold_ms": THRESHOLD_NS / 1e6, "workload_fixture": FIXTURE, "runs": rows, "pass": passed}
    text = json.dumps(summary, indent=2) + "\n"
    if args.summary:
        args.summary.write_text(text)
    print(text, end="")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
