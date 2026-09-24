import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

from compare import main


class ValueGateTest(unittest.TestCase):
    def test_binary_gain_does_not_bypass_missing_claimed_rss_measurements(self):
        def workload(rss):
            return {
                "status": "pass", "samples": 30,
                "raw_samples": [{"duration_ns": 100, "peak_rss_bytes": rss, "peak_rss_method": "fixture"} for _ in range(30)],
                "p95_duration_ns": 100, "peak_rss_status": "complete",
                "median_peak_rss_bytes": rss,
            }

        def binary(rss):
            sample = workload(rss)
            sample_fetch = dict(sample, semantic_contract={"negative_control": {"rejected": True}})
            return {
                "identity": {"sha256": "a" * 64},
                "cli": sample, "mcp": sample, "daemon": dict(sample, steady_state_100_frames=sample),
                "fetch": sample_fetch, "cli_variants": {"help": sample, "config": sample},
            }

        report = {
            "schema_version": 3, "report_version": "rust016-benchmark-v3",
            "source_revision": "fixture", "gate": "pass", "runs_per_workload": 30,
            "cache_policy": "no_cache=true for fetch requests; fresh HOME/XDG roots per process probe",
            "p95_calculation": "nearest-rank: sorted_samples[ceil(0.95*n)-1]",
            "reference_size_bytes": 100, "candidate_size_bytes": 70,
            "binaries": {"go": binary(100), "rust": binary(80)},
        }
        baseline = {"release": {"current_build_uncompressed_bytes": 1000},
                    "measurements": {"version_peak_rss": {"median_bytes": 100}}}
        with tempfile.TemporaryDirectory() as directory:
            baseline_path = Path(directory, "baseline.json")
            report_path = Path(directory, "report.json")
            baseline_path.write_text(json.dumps(baseline))
            report_path.write_text(json.dumps(report))
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main([str(baseline_path), str(report_path)]), 0)
                report["binaries"]["rust"]["cli"]["raw_samples"][0]["peak_rss_bytes"] = None
                report_path.write_text(json.dumps(report))
                self.assertEqual(main([str(baseline_path), str(report_path)]), 1)

    def test_binary_size_gain_does_not_bypass_the_claimed_perf001_rss_gate(self):
        def workload(rss):
            return {
                "status": "pass", "samples": 30,
                "raw_samples": [{"duration_ns": 100, "peak_rss_bytes": rss, "peak_rss_method": "fixture"} for _ in range(30)],
                "p95_duration_ns": 100, "peak_rss_status": "complete",
                "median_peak_rss_bytes": rss,
            }

        def binary(rss):
            sample = workload(rss)
            return {
                "identity": {"sha256": "a" * 64},
                "cli": sample, "mcp": sample, "daemon": dict(sample, steady_state_100_frames=sample),
                "fetch": dict(sample, semantic_contract={"negative_control": {"rejected": True}}),
                "cli_variants": {"help": sample, "config": sample},
            }

        report = {
            "schema_version": 3, "report_version": "rust016-benchmark-v3",
            "source_revision": "fixture", "gate": "pass", "runs_per_workload": 30,
            "cache_policy": "no_cache=true for fetch requests; fresh HOME/XDG roots per process probe",
            "p95_calculation": "nearest-rank: sorted_samples[ceil(0.95*n)-1]",
            "reference_size_bytes": 100, "candidate_size_bytes": 70,
            "binaries": {"go": binary(100), "rust": binary(90)},
        }
        baseline = {"release": {"current_build_uncompressed_bytes": 1000},
                    "measurements": {"version_peak_rss": {"median_bytes": 100}}}
        with tempfile.TemporaryDirectory() as directory:
            baseline_path = Path(directory, "baseline.json")
            report_path = Path(directory, "report.json")
            baseline_path.write_text(json.dumps(baseline))
            report_path.write_text(json.dumps(report))
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(main([str(baseline_path), str(report_path)]), 1)
            comparison = json.loads(output.getvalue())
            self.assertEqual(comparison["value_gate"]["satisfied_by"], ["binary_size"])
            self.assertTrue(any("RSS exceeds 80%" in reason for reason in comparison["reasons"]))


if __name__ == "__main__":
    unittest.main()
