"""Failure controls for the Rust coverage floor and its complete denominator."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


report = load("report")


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
