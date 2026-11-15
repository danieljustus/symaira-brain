#!/usr/bin/env python3
"""Evaluate PERF-001..PERF-004 from `perf_evidence.py` rounds and the frozen Go baseline.

Go is gone (#1003); the reference is `docs/rust-port/baseline.json`
(`measurements.rust_001_portbench`, 30 runs per workload, plus `release` sizes).
Rules (fixed before looking at the numbers):

* PERF-001: for version-json, root-help, config-show-json the p95 of the Rust
  distribution must be <= 110% of the Go p95, and median peak RSS <= 80% of the Go
  median RSS. A workload passes when the *median across rounds* of its p95 meets the
  limit; the per-round pass count and the worst round are reported alongside.
* PERF-002: mcp-initialize-list p95 <= 110% of Go (same aggregation) and no growth
  in threads/file descriptors and bounded RSS growth over the 100-frame steady-state
  runs. A paired Go reference for the daemon/tools-call frames does not exist in the
  baseline, so those frames can only be reported, not compared.
* PERF-003: fetch p95 (daemon fetch.url, loopback fixture) <= the fixed 6.4834 ms
  ceiling (recorded Go p95 5.894 ms x 1.10, #773). A real-Chrome fixture flow has no
  Go reference in the baseline and is not measured here.
* PERF-004: >= 20% smaller uncompressed binary OR >= 20% lower median RSS than Go,
  *and* every no-regression gate above passing.
"""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path

P95_LIMIT = 1.10
RSS_LIMIT = 0.80
FETCH_CEILING_NS = 6_483_400
STEADY_RSS_GROWTH_LIMIT = 4 * 1024 * 1024  # bytes over 200 frames
PROCESS_WORKLOADS = ("version-json", "root-help", "config-show-json", "mcp-initialize-list")


def per_workload(rounds: list[dict], baseline: dict) -> dict:
    out = {}
    for name in PROCESS_WORKLOADS:
        go = baseline["measurements"]["rust_001_portbench"]["workloads"][name]
        p95s = [r["process"][name]["p95_ns"] for r in rounds]
        medians = [r["process"][name]["median_ns"] for r in rounds]
        rss = [r["process"][name]["median_rss_bytes"] for r in rounds]
        limit = go["p95_ns"] * P95_LIMIT
        median_p95 = statistics.median(p95s)
        out[name] = {
            "go_p95_ns": go["p95_ns"], "go_median_ns": go["median_ns"], "go_median_rss_bytes": go["median_rss_bytes"],
            "p95_limit_ns": int(limit),
            "rust_p95_ns_per_round": p95s,
            "rust_median_ns_per_round": medians,
            "rust_p95_median_of_rounds_ns": int(median_p95),
            "rust_p95_best_round_ns": min(p95s),
            "rust_p95_worst_round_ns": max(p95s),
            "rounds_p95_within_limit": sum(1 for value in p95s if value <= limit),
            "rounds": len(p95s),
            "p95_ratio_vs_go": round(median_p95 / go["p95_ns"], 4),
            "p95_pass": median_p95 <= limit,
            "rust_median_rss_bytes": int(statistics.median(rss)),
            "rss_ratio_vs_go": round(statistics.median(rss) / go["median_rss_bytes"], 4),
            "rss_pass": statistics.median(rss) <= go["median_rss_bytes"] * RSS_LIMIT,
        }
    return out


def steady(rounds: list[dict]) -> dict:
    def grow(key: str, sub: str) -> list[int]:
        return [r[key]["after"][sub] - r[key]["before"][sub] for r in rounds]

    mcp_daemon_growth = [r["mcp_steady"]["after"]["daemon"]["rss_bytes"] - r["mcp_steady"]["before"]["daemon"]["rss_bytes"] for r in rounds]
    mcp_growth = [r["mcp_steady"]["after"]["mcp"]["rss_bytes"] - r["mcp_steady"]["before"]["mcp"]["rss_bytes"] for r in rounds]
    daemon_growth = grow("daemon_steady", "rss_bytes")
    threads = [r["daemon_steady"]["after"]["threads"] - r["daemon_steady"]["before"]["threads"] for r in rounds]
    fds = [r["daemon_steady"]["after"]["open_fds"] - r["daemon_steady"]["before"]["open_fds"] for r in rounds]
    mcp_threads = [r["mcp_steady"]["after"]["daemon"]["threads"] - r["mcp_steady"]["before"]["daemon"]["threads"] for r in rounds]
    ok = all(r["mcp_steady"]["tools_call_semantic_ok"] == 100 and r["daemon_steady"]["fetch_semantic_ok"] == 100 for r in rounds)

    def med(key: str, sub: str) -> float:
        return statistics.median(r[key][sub]["median_ns"] for r in rounds)

    def p95s(key: str, sub: str) -> list[int]:
        return [r[key][sub]["p95_ns"] for r in rounds]

    bounded = (max(mcp_daemon_growth + mcp_growth + daemon_growth) <= STEADY_RSS_GROWTH_LIMIT
               and not any(threads) and not any(fds) and not any(mcp_threads))
    return {
        "frames_per_series": 100,
        "semantic_checks_all_passed": ok,
        "rss_growth_bytes_over_200_frames": {
            "mcp_process": mcp_growth, "mcp_spawned_daemon": mcp_daemon_growth, "static_daemon": daemon_growth},
        "thread_growth_static_daemon": threads, "fd_growth_static_daemon": fds,
        "thread_growth_mcp_spawned_daemon": mcp_threads,
        "growth_limit_bytes": STEADY_RSS_GROWTH_LIMIT,
        "bounded_no_unbounded_growth": bounded,
        "mcp_ping_p95_ns_per_round": p95s("mcp_steady", "ping"),
        "mcp_tools_call_fetch_batch_p95_ns_per_round": p95s("mcp_steady", "tools_call_fetch_batch"),
        "mcp_tools_call_fetch_batch_median_ns": int(med("mcp_steady", "tools_call_fetch_batch")),
        "daemon_ping_p95_ns_per_round": p95s("daemon_steady", "ping"),
        "daemon_fetch_url_p95_ns_per_round": p95s("daemon_steady", "fetch_url"),
        "daemon_fetch_url_median_ns": int(med("daemon_steady", "fetch_url")),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--rounds", type=Path, nargs="+", required=True, help="perf_evidence.py outputs")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    baseline = json.loads(args.baseline.read_text())
    rounds: list[dict] = []
    binary = None
    for path in args.rounds:
        report = json.loads(path.read_text())
        binary = binary or report["binary"]
        if report["binary"]["sha256"] != binary["sha256"]:
            raise SystemExit("rounds measure different binaries")
        rounds += report["rounds"]
    if binary is None:
        raise SystemExit("no rounds supplied")
    for index, item in enumerate(rounds, 1):
        item["round"] = index
    workloads = per_workload(rounds, baseline)
    state = steady(rounds)
    fetch_p95s = [r["daemon_steady"]["fetch_url"]["p95_ns"] for r in rounds]
    go_size = baseline["release"]["v0_8_0_darwin_arm64_uncompressed_bytes"]
    size_reduction = (1 - binary["size_bytes"] / go_size) * 100
    rss_reductions = {name: (1 - w["rust_median_rss_bytes"] / w["go_median_rss_bytes"]) * 100 for name, w in workloads.items()}
    perf1 = all(workloads[n]["p95_pass"] and workloads[n]["rss_pass"] for n in PROCESS_WORKLOADS[:3])
    perf2_mcp = workloads["mcp-initialize-list"]["p95_pass"]
    fetch_median_of_rounds = statistics.median(fetch_p95s)
    verdict = {
        "PERF-001": {"threshold": "p95 <= 110% Go and median RSS <= 80% Go", "measured_pass": perf1},
        "PERF-002": {"threshold": "p95 <= 110% Go; no unbounded task/channel growth",
                     "mcp_initialize_list_p95_pass": perf2_mcp,
                     "steady_state_bounded": state["bounded_no_unbounded_growth"],
                     "go_reference_for_100_frame_daemon_workload": "absent from baseline.json",
                     "measured_pass": False},
        "PERF-003": {"threshold": "fetch p95 <= 6.4834 ms (Go 5.894 ms x 1.10) plus real-Chrome fixture flow",
                     "fetch_p95_median_of_rounds_ns": int(fetch_median_of_rounds),
                     "fetch_p95_pass": fetch_median_of_rounds <= FETCH_CEILING_NS,
                     "chrome_flow_go_reference": "absent from baseline.json; not measured",
                     "measured_pass": False},
        "PERF-004": {"threshold": ">=20% smaller binary OR >=20% lower median RSS, with all no-regression gates passing",
                     "size_reduction_percent_vs_go_v0_8_0": round(size_reduction, 2),
                     "median_rss_reduction_percent_by_workload": {k: round(v, 2) for k, v in rss_reductions.items()},
                     "value_criterion_met": size_reduction >= 20 or any(v >= 20 for v in rss_reductions.values()),
                     "all_no_regression_gates_pass": False, "measured_pass": False},
    }
    result = {"binary": binary, "go_baseline": {
        "source": "docs/rust-port/baseline.json", "captured_at": baseline["captured_at"],
        "oracle": baseline["oracle"]["commit"], "binary_size_bytes": go_size},
        "rounds": len(rounds), "round_loads_1min": [[round(r["load1_at_start"], 1), round(r["load1_at_end"], 1)] for r in rounds],
        "process_workloads": workloads, "steady_state": state, "verdict": verdict,
        "control_echo": {"median_ns_per_round": [r["process"]["control-echo"]["median_ns"] for r in rounds],
                         "p95_ns_per_round": [r["process"]["control-echo"]["p95_ns"] for r in rounds]}
        if "control-echo" in rounds[0]["process"] else None}
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"verdict": verdict, "p95_ratio_vs_go": {k: v["p95_ratio_vs_go"] for k, v in workloads.items()},
                      "rounds_within_limit": {k: f"{v['rounds_p95_within_limit']}/{v['rounds']}" for k, v in workloads.items()}}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
