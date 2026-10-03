#!/usr/bin/env python3
"""Validate the full LLVM denominator and report per-crate coverage diagnostics."""
import argparse
import hashlib
import json
import platform
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def summarize(report, policy, members=None):
    if report.get("type") != "llvm.coverage.json.export" or len(report["data"]) != 1:
        raise ValueError("expected one complete LLVM workspace export")
    leaked = list((ROOT / "rust").glob("*/default_*.profraw"))
    if leaked:
        raise ValueError("profiling destination lost by a Rust subprocess")
    data = report["data"][0]
    if members is None:
        members = tomllib.loads((ROOT / policy["workspace"]).read_text())["workspace"]["members"]
    crates = {member: {"covered": 0, "count": 0, "files": 0} for member in members}
    observed = set()
    totals = {metric: {"covered": 0, "count": 0} for metric in ("lines", "functions", "regions")}
    files = []
    for file in data["files"]:
        path = Path(file["filename"]).resolve().relative_to(ROOT).as_posix()
        if path in observed:
            raise ValueError(f"duplicate source file: {path}")
        observed.add(path)
        for metric, aggregate in totals.items():
            counts = file["summary"][metric]
            covered, count = counts["covered"], counts["count"]
            if type(covered) is not int or type(count) is not int or not 0 <= covered <= count:
                raise ValueError(f"invalid {metric} counts: {path}")
            aggregate["covered"] += covered
            aggregate["count"] += count
        for member, crate in crates.items():
            if path.startswith(member + "/"):
                crate["covered"] += file["summary"]["lines"]["covered"]
                crate["count"] += file["summary"]["lines"]["count"]
                crate["files"] += 1
                break
        files.append({"path": path, "summary": file["summary"]})
    if not files or any(crate["count"] == 0 for crate in crates.values()):
        raise ValueError("empty coverage or missing workspace member")
    for metric, counts in totals.items():
        raw = data["totals"][metric]
        if counts["count"] <= 0 or counts != {key: raw[key] for key in counts}:
            raise ValueError(f"inconsistent full {metric} denominator")
    for counts in [*totals.values(), *crates.values()]:
        counts["percent"] = 100 * counts["covered"] / counts["count"]
    return dict(policy=policy, platform=f"{platform.system()} {platform.machine()}",
                totals=totals, crates=crates, files=files)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--print-floor", action="store_true")
    parser.add_argument("--print-tool-version", action="store_true")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--summary", type=Path)
    parser.add_argument("--github-summary", type=Path)
    args = parser.parse_args()
    policy = json.loads((ROOT / "rust/coverage-policy.json").read_text())
    if args.print_floor:
        print(policy["minimum_percent"])
        return
    if args.print_tool_version:
        print(policy["cargo_llvm_cov"])
        return
    if args.report is None or args.summary is None:
        parser.error("--report and --summary are required")
    try:
        summary = summarize(json.loads(args.report.read_text()), policy)
    except (ValueError, KeyError, TypeError, OSError) as error:
        raise SystemExit(f"invalid coverage evidence: {error}") from error
    summary["report_sha256"] = hashlib.sha256(args.report.read_bytes()).hexdigest()
    summary["source_commit"] = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    args.summary.write_text(json.dumps(summary, indent=2) + "\n")
    lines = summary["totals"]["lines"]
    output = (f"Rust workspace line coverage: {lines['covered']}/{lines['count']} "
              f"= {lines['percent']:.2f}% (floor {policy['minimum_percent']:.1f}%)\n")
    output += "\n| Crate | Covered lines | Total lines | Coverage |\n|---|---:|---:|---:|\n"
    for member, counts in summary["crates"].items():
        output += (f"| {member.split('/')[-1]} | {counts['covered']} | {counts['count']} "
                   f"| {counts['percent']:.2f}% |\n")
    print(output)
    if args.github_summary:
        with args.github_summary.open("a") as stream:
            stream.write(output)
    if lines["percent"] < policy["minimum_percent"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
