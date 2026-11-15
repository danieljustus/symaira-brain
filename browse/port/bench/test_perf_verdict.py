#!/usr/bin/env python3
"""Unit tests for the PERF verdict helpers (synthetic inputs, no measurement claim)."""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import perf_evidence  # noqa: E402
import perf_verdict  # noqa: E402


def baseline() -> dict:
    return {"measurements": {"rust_001_portbench": {"workloads": {
        name: {"median_ns": 8_000_000, "p95_ns": 10_000_000, "median_rss_bytes": 25_000_000}
        for name in perf_verdict.PROCESS_WORKLOADS}}}}


def round_with(p95_ns: int, rss: int) -> dict:
    return {"process": {name: {"p95_ns": p95_ns, "median_ns": p95_ns // 2, "median_rss_bytes": rss}
                        for name in perf_verdict.PROCESS_WORKLOADS}}


class PerfVerdictTests(unittest.TestCase):
    def test_nearest_rank_p95(self) -> None:
        self.assertEqual(perf_evidence.p95(list(range(1, 31))), 29)
        self.assertEqual(perf_evidence.p95([5]), 5)

    def test_median_of_rounds_gate(self) -> None:
        rounds = [round_with(10_900_000, 9_000_000), round_with(11_000_000, 9_000_000), round_with(90_000_000, 9_000_000)]
        result = perf_verdict.per_workload(rounds, baseline())["version-json"]
        self.assertTrue(result["p95_pass"])  # median 11.0 ms is exactly the 110 % limit
        self.assertEqual(result["rounds_p95_within_limit"], 2)
        self.assertTrue(result["rss_pass"])

    def test_slow_median_fails(self) -> None:
        rounds = [round_with(11_000_001, 9_000_000)] * 3
        self.assertFalse(perf_verdict.per_workload(rounds, baseline())["root-help"]["p95_pass"])

    def test_rss_over_80_percent_fails(self) -> None:
        rounds = [round_with(1_000_000, 21_000_000)] * 3
        self.assertFalse(perf_verdict.per_workload(rounds, baseline())["config-show-json"]["rss_pass"])


if __name__ == "__main__":
    unittest.main()
