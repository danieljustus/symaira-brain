"""Failure controls for the coverage floor and preserved Go denominator."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


report = load("report")
go = load("go_oracles")


class CoverageControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.baseline = json.loads((ROOT / "migration/evidence/coverage-682/linux-baseline.json").read_text())
        cls.members = cls.baseline["provenance"]["workspace_members"]
        cls.policy = json.loads((ROOT / "rust/coverage-policy.json").read_text())

    def test_real_baseline_preserves_all_file_totals(self):
        result = report.summarize(self.baseline, self.policy, self.members)
        self.assertEqual(result["totals"]["lines"]["count"], 41701)
        self.assertEqual(result["totals"]["lines"]["covered"], 33698)
        self.assertEqual(len(result["crates"]), 18)

    def test_deleted_source_and_empty_reports_cannot_pass(self):
        altered = copy.deepcopy(self.baseline)
        altered["data"][0]["files"].pop()
        with self.assertRaisesRegex(ValueError, "denominator"):
            report.summarize(altered, self.policy, self.members)
        altered["data"][0]["files"] = []
        with self.assertRaisesRegex(ValueError, "empty"):
            report.summarize(altered, self.policy, self.members)

    def test_omitted_member_fails_even_with_consistent_reduced_totals(self):
        altered = copy.deepcopy(self.baseline)
        removed = [f for f in altered["data"][0]["files"] if f["filename"].startswith("rust/symbrain-memory/")]
        altered["data"][0]["files"] = [f for f in altered["data"][0]["files"] if f not in removed]
        for metric in ("lines", "functions", "regions"):
            for key in ("covered", "count"):
                altered["data"][0]["totals"][metric][key] -= sum(f["summary"][metric][key] for f in removed)
        with self.assertRaisesRegex(ValueError, "missing workspace member"):
            report.summarize(altered, self.policy, self.members)

    def test_go_merge_keeps_unexecuted_blocks_and_rejects_scope_changes(self):
        baseline = {"product.go:1.1,2.1": (3, 1), "scripts/unused.go:1.1,2.1": (9, 0)}
        merged = go.merge_profiles("mode: atomic", baseline, {"product.go:1.1,2.1": (3, 2)})
        self.assertEqual(merged["scripts/unused.go:1.1,2.1"], (9, 0))
        self.assertEqual(go.counts(merged)["count"], 12)
        self.assertEqual(merged["product.go:1.1,2.1"], (3, 3))
        for extra in ({"new.go:1.1,2.1": (1, 1)}, {"product.go:1.1,2.1": (4, 1)}):
            with self.assertRaisesRegex(ValueError, "denominator"):
                go.merge_profiles("mode: atomic", baseline, extra)

    def test_go_set_mode_does_not_inflate_counts(self):
        merged = go.merge_profiles("mode: set", {"a.go:1.1,2.1": (3, 1)}, {"a.go:1.1,2.1": (3, 1)})
        self.assertEqual(merged["a.go:1.1,2.1"], (3, 1))

    def test_go_empty_duplicate_and_negative_profiles_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "coverage.out"
            for text in ("mode: atomic\n", "mode: atomic\na.go:1.1,2.1 3 0\na.go:1.1,2.1 3 1\n",
                         "mode: atomic\na.go:1.1,2.1 3 -1\n"):
                path.write_text(text)
                with self.assertRaises(ValueError):
                    go.read_profile(path)
