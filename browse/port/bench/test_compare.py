import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

from compare import main


class ValueGateTest(unittest.TestCase):
    def test_unmeasured_rss_cannot_replace_binary_size_evidence(self):
        sample = {
            "status": "pass", "samples": 30,
            "raw_samples": [{"duration_ns": 100} for _ in range(30)],
            "p95_duration_ns": 100,
        }
        sample_fetch = dict(sample, semantic_contract={"negative_control": {"rejected": True}})
        binary = {
            "identity": {"sha256": "fixture"},
            "cli": sample, "mcp": sample, "daemon": dict(sample, steady_state_100_frames=sample),
            "fetch": sample_fetch, "cli_variants": {"help": sample, "config": sample},
        }
        report = {
            "schema_version": 2, "report_version": "rust016-benchmark-v2",
            "source_revision": "fixture", "gate": "pass", "runs_per_workload": 30,
            "cache_policy": "no_cache=true for fetch requests; fresh HOME/XDG roots per process probe",
            "p95_calculation": "nearest-rank: sorted_samples[ceil(0.95*n)-1]",
            "reference_size_bytes": 100, "candidate_size_bytes": 100,
            "candidate_median_peak_rss_bytes": 1,
            "binaries": {"go": binary, "rust": binary},
        }
        baseline = {"release": {"current_build_uncompressed_bytes": 1000},
                    "measurements": {"version_peak_rss": {"median_bytes": 100}}}
        with tempfile.TemporaryDirectory() as directory:
            baseline_path = Path(directory, "baseline.json")
            report_path = Path(directory, "report.json")
            baseline_path.write_text(json.dumps(baseline))
            report_path.write_text(json.dumps(report))
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main([str(baseline_path), str(report_path)]), 1)
                report["candidate_size_bytes"] = 70
                report_path.write_text(json.dumps(report))
                self.assertEqual(main([str(baseline_path), str(report_path)]), 0)


if __name__ == "__main__":
    unittest.main()
