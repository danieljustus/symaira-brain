#!/usr/bin/env python3
"""Measure the Rust real-Chrome local-fixture flow (PERF-003) on one native host.

Go was removed in #1003, so there is no paired reference. The maintainer
decision of 2026-11-16 (#780) records these Rust numbers as the new PERF-003
Chrome baseline instead. The flow, fixture, isolation and p95 rule are the
ones of the historical paired harness (`chrome_pair.py`), imported unchanged.
"""
from __future__ import annotations

import argparse
import json
import platform
import statistics
import subprocess
import sys
import tempfile
import threading
from http.server import ThreadingHTTPServer
from pathlib import Path
from typing import Any, Sequence

from chrome_pair import (
    FIXTURE_TITLE,
    FIXTURE_TOKEN,
    FixtureHandler,
    flow,
    identity,
    nearest_rank,
    remove_owned_tempdir,
    sha256,
    summarize_stage,
)


def measure(args: argparse.Namespace) -> dict[str, Any]:
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=args.repo, capture_output=True, text=True, check=True,
    ).stdout.strip()
    report: dict[str, Any] = {
        "schema_version": 1, "report_version": "perf003-real-chrome-rust-v1",
        "source_revision": revision,
        "host": {"system": platform.system(), "machine": platform.machine()},
        "fixture": {"id": "perf003-local-chrome-html-v1", "title": FIXTURE_TITLE,
                    "content_token": FIXTURE_TOKEN, "flow": ["open", "read"]},
        "chrome": {"version": args.chrome_version, "binary_sha256": sha256(args.chrome)},
        "binary": identity(args.rust), "runs": args.runs,
        "p95_calculation": "nearest-rank: sorted_samples[ceil(0.95*n)-1]",
    }
    server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{server.server_port}/fixture.html"
    samples: list[dict[str, Any]] = []
    try:
        for index in range(args.runs):
            # Short root on macOS: the daemon socket path must stay under the
            # sun_path limit.
            root = Path(tempfile.mkdtemp(prefix="p3-r-", dir="/tmp" if sys.platform == "darwin" else None))
            try:
                sample = flow(args.rust, "rust", args.chrome, args.chrome_launcher, url, root, index)
            finally:
                remove_owned_tempdir(root)
            samples.append(sample)
            if sample["status"] != "pass":
                break
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()
    passing = [int(item["duration_ns"]) for item in samples if item.get("status") == "pass"]
    report.update(
        status="measured" if len(passing) == args.runs else "error",
        samples=samples, sample_count=len(passing),
        p95_duration_ns=nearest_rank(passing) if passing else None,
        median_duration_ns=int(statistics.median(passing)) if passing else None,
        stages={
            "open_cli": summarize_stage(samples, "open_cli_duration_ns"),
            "read_cli": summarize_stage(samples, "read_cli_duration_ns"),
        },
    )
    return report


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--chrome", type=Path, required=True)
    parser.add_argument("--chrome-launcher", type=Path)
    parser.add_argument("--chrome-version")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--runs", type=int, default=30)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.runs < 1:
        parser.error("--runs must be positive")
    report = measure(args)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    summary = {key: report[key] for key in ("status", "sample_count", "p95_duration_ns", "median_duration_ns")}
    print(json.dumps(summary, indent=2))
    return 0 if report["status"] == "measured" else 1


if __name__ == "__main__":
    raise SystemExit(main())
