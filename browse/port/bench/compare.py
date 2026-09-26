#!/usr/bin/env python3
"""Compare a RUST-016 benchmark report against the measured Go baseline.

The command is conservative: it reports a BLOCK when required workloads are
missing, when the candidate has no representative evidence, or when the value
thresholds are not met.  It does not turn an incomplete candidate into a cutover
recommendation.
"""
from __future__ import annotations

import argparse
import json
import statistics
import sys
from pathlib import Path
from typing import Any, Sequence

VALUE_SIZE_REDUCTION = 20.0
MAX_P95_REGRESSION = 10.0
REQUIRED_WORKLOADS = ("cli", "mcp", "daemon", "fetch")
CLI_VARIANTS = ("help", "config")


def pct(reference: float, candidate: float) -> float:
    if reference <= 0:
        return 0.0
    return (candidate / reference - 1.0) * 100.0


def load(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} is not a JSON object")
    return value


def nearest_rank(samples: list[dict[str, Any]]) -> float:
    values = sorted(float(item["duration_ns"]) for item in samples)
    return values[max(0, (len(values) * 95 + 99) // 100 - 1)]


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("report", type=Path)
    parser.add_argument("--reference", default="go")
    parser.add_argument("--candidate", default="rust")
    args = parser.parse_args(argv)
    try:
        baseline = load(args.baseline)
        report = load(args.report)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"BLOCK: {error}", file=sys.stderr)
        return 1

    result: dict[str, Any] = {"schema_version": 3, "gate": "blocked", "workloads": {}, "reasons": []}
    if report.get("schema_version") != 3 or report.get("report_version") != "rust016-benchmark-v3":
        result["reasons"].append("versioned rust016-benchmark-v3 report is required")
    if report.get("cache_policy") != "no_cache=true for fetch requests; fresh HOME/XDG roots per process probe":
        result["reasons"].append("required cache policy is missing")
    if report.get("runs_per_workload") != 30:
        result["reasons"].append("exactly 30 runs per workload are required")
    if report.get("p95_calculation") != "nearest-rank: sorted_samples[ceil(0.95*n)-1]":
        result["reasons"].append("declared p95 calculation is missing or changed")
    binaries = report.get("binaries")
    if not isinstance(binaries, dict):
        result["reasons"].append("benchmark report has no binaries object")
        print(json.dumps(result, indent=2))
        return 1
    reference = binaries.get(args.reference)
    candidate = binaries.get(args.candidate)
    if not isinstance(reference, dict) or not isinstance(candidate, dict):
        result["reasons"].append("paired Go and Rust benchmark results are required")
        print(json.dumps(result, indent=2))
        return 1

    if report.get("gate") != "pass":
        result["reasons"].append("benchmark report did not pass its paired execution gate")
    for label, value in ((args.reference, reference), (args.candidate, candidate)):
        identity = value.get("identity")
        if not isinstance(identity, dict) or not identity.get("sha256"):
            result["reasons"].append(f"{label} binary identity digest is required")
    if not report.get("source_revision"):
        result["reasons"].append("source checkout revision is required")
    workload_pairs = {name: (reference.get(name), candidate.get(name)) for name in REQUIRED_WORKLOADS}
    left_variants = reference.get("cli_variants") if isinstance(reference.get("cli_variants"), dict) else {}
    right_variants = candidate.get("cli_variants") if isinstance(candidate.get("cli_variants"), dict) else {}
    workload_pairs.update({f"cli/{name}": (left_variants.get(name), right_variants.get(name)) for name in CLI_VARIANTS})
    workload_pairs["daemon/steady_state_100_frames"] = (
        reference.get("daemon", {}).get("steady_state_100_frames"),
        candidate.get("daemon", {}).get("steady_state_100_frames"),
    )
    comparable = []
    for name, (left, right) in workload_pairs.items():
        if not isinstance(left, dict) or left.get("status") != "pass":
            result["reasons"].append(f"{args.reference} workload {name} is not executable")
            continue
        if not isinstance(right, dict) or right.get("status") != "pass":
            result["reasons"].append(f"{args.candidate} workload {name} is not executable")
            continue
        if left.get("samples") != 30 or right.get("samples") != 30:
            result["reasons"].append(
                f"workload {name} does not contain 30 complete paired samples"
            )
            continue
        if not isinstance(left.get("raw_samples"), list) or not isinstance(right.get("raw_samples"), list) or len(left["raw_samples"]) != 30 or len(right["raw_samples"]) != 30:
            result["reasons"].append(f"workload {name} raw samples are missing or incomplete")
            continue
        try:
            left_p95 = nearest_rank(left["raw_samples"])
            right_p95 = nearest_rank(right["raw_samples"])
        except (KeyError, TypeError, ValueError):
            result["reasons"].append(f"workload {name} contains invalid raw samples")
            continue
        if left.get("p95_duration_ns") != left_p95 or right.get("p95_duration_ns") != right_p95:
            result["reasons"].append(f"workload {name} declared p95 does not match raw samples")
            continue
        change = pct(left_p95, right_p95)
        result["workloads"][name] = {"p95_change_percent": change}
        if name == "fetch":
            for label, workload in ((args.reference, left), (args.candidate, right)):
                semantic = workload.get("semantic_contract", {})
                negative = semantic.get("negative_control", {}) if isinstance(semantic, dict) else {}
                if not isinstance(negative, dict) or negative.get("rejected") is not True:
                    result["reasons"].append(f"{label} fetch negative control was not rejected")
            result["workloads"][name]["hard_gate"] = "<=10%"
        comparable.append(change)

    rss_medians: dict[str, int] = {}
    rss_methods: dict[str, str] = {}
    for label, value in ((args.reference, reference), (args.candidate, candidate)):
        startup = value.get("cli")
        samples = startup.get("raw_samples") if isinstance(startup, dict) else None
        if (
            not isinstance(startup, dict)
            or startup.get("peak_rss_status") != "complete"
            or not isinstance(samples, list)
            or len(samples) != 30
        ):
            result["reasons"].append(f"{label} CLI startup peak RSS requires 30 complete samples")
            continue
        rss_values = [item.get("peak_rss_bytes") for item in samples if isinstance(item, dict)]
        methods = [item.get("peak_rss_method") for item in samples if isinstance(item, dict)]
        if (
            len(rss_values) != 30
            or any(not isinstance(item, int) or item <= 0 for item in rss_values)
            or any(not isinstance(method, str) or not method for method in methods)
            or len(set(methods)) != 1
        ):
            result["reasons"].append(f"{label} CLI startup RSS contains missing, zero, or untyped measurements")
            continue
        median_rss = int(statistics.median(rss_values))
        if startup.get("median_peak_rss_bytes") != median_rss:
            result["reasons"].append(f"{label} CLI startup RSS median does not match raw samples")
            continue
        rss_medians[label] = median_rss
        rss_methods[label] = methods[0]
    if len(rss_medians) == 2:
        if rss_methods[args.reference] != rss_methods[args.candidate]:
            result["reasons"].append("Go and Rust CLI RSS were measured by different OS mechanisms")
        rss_change = pct(rss_medians[args.reference], rss_medians[args.candidate])
        result["cli_peak_rss"] = {
            f"{args.reference}_median_bytes": rss_medians[args.reference],
            f"{args.candidate}_median_bytes": rss_medians[args.candidate],
            "change_percent": rss_change,
            "measurement_method": rss_methods[args.reference],
            "hard_gate": "Rust median <=80% of Go median",
        }
        if rss_medians[args.candidate] > rss_medians[args.reference] * 0.8:
            result["reasons"].append("Rust CLI startup median peak RSS exceeds 80% of the Go median")

    baseline_size = report.get("reference_size_bytes")
    candidate_size = report.get("candidate_size_bytes")
    size_reduction = None
    if isinstance(baseline_size, (int, float)) and isinstance(candidate_size, (int, float)) and baseline_size:
        size_reduction = (1.0 - candidate_size / baseline_size) * 100.0
        result["size_reduction_percent"] = size_reduction

    value_reasons = []
    if size_reduction is not None and size_reduction >= VALUE_SIZE_REDUCTION:
        value_reasons.append("binary_size")
    rss_reduction = None
    if len(rss_medians) == 2:
        rss_reduction = (1.0 - rss_medians[args.candidate] / rss_medians[args.reference]) * 100.0
        if rss_medians[args.candidate] * 100 <= rss_medians[args.reference] * (100 - VALUE_SIZE_REDUCTION):
            value_reasons.append("median_rss")
    result["value_gate"] = {
        "satisfied": bool(value_reasons),
        "satisfied_by": value_reasons,
        "binary_size_reduction_percent": size_reduction,
        "median_rss_reduction_percent": rss_reduction,
    }

    p95_ok = len(comparable) == len(workload_pairs) and max(comparable, default=float("inf")) <= MAX_P95_REGRESSION
    value_ok = bool(value_reasons)
    if not comparable:
        result["reasons"].append("no complete representative workload pair")
    elif len(comparable) != len(workload_pairs):
        result["reasons"].append("every representative workload must have a Go/Rust pair")
    if not p95_ok:
        result["reasons"].append("p95 regression gate is missing or exceeds 10%")
    if not value_ok:
        result["reasons"].append("neither binary size nor median RSS is improved by at least 20%")
    if not result["reasons"]:
        result["gate"] = "pass"
    print(json.dumps(result, indent=2))
    return 0 if result["gate"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
