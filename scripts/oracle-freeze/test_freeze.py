#!/usr/bin/env python3
"""Negative controls for the Go-free fixture gate (run with python3 -m unittest)."""
from __future__ import annotations

import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import freeze  # noqa: E402

REAL = freeze.ROOT


class FreezeGate(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="oracle-freeze-test."))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        manifest = freeze.load(REAL / "migration/evidence/oracle-freeze-781/manifest.json")
        for rel in ["scripts/oracle-freeze/registry.json", "migration/evidence/oracle-freeze-781/manifest.json"]:
            (self.tmp / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REAL / rel, self.tmp / rel)
        self.paths = [f["path"] for g in manifest["groups"] for f in g["fixtures"]]
        for rel in self.paths:
            (self.tmp / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REAL / rel, self.tmp / rel)

    def failures(self) -> str:
        with self.assertRaises(freeze.FreezeError) as ctx:
            freeze.verify(self.tmp)
        return "\n".join(ctx.exception.failures)

    def test_pristine_copy_passes_and_counts_everything(self) -> None:
        report = freeze.verify(self.tmp)
        self.assertEqual(report["fixtures"], len(self.paths))
        self.assertGreaterEqual(report["fixtures"], 100)

    def test_real_repository_passes(self) -> None:
        self.assertEqual(freeze.verify()["fixtures"], len(self.paths))

    def test_mutated_json_field_is_rejected_through_the_real_function(self) -> None:
        target = self.tmp / "rust/symbrain-policy/tests/fixtures/oracle_expectations.json"
        document = json.loads(target.read_text())
        case = document["policy_cases"][0]
        case["exposed"] = list(case.get("exposed") or []) + ["tampered_tool"]
        target.write_text(json.dumps(document, indent=2) + "\n")
        message = self.failures()
        self.assertIn("sha256 mismatch for rust/symbrain-policy/tests/fixtures/oracle_expectations.json", message)

    def test_trailing_byte_is_rejected(self) -> None:
        target = self.tmp / "rust/symbrain-audit/tests/fixtures/oracle_expectations.json"
        target.write_bytes(target.read_bytes() + b"\n")
        self.assertIn("mismatch", self.failures())

    def test_missing_fixture_fails_instead_of_skipping(self) -> None:
        (self.tmp / "rust/symbrain-catalog/tests/fixtures/oracle_expectations.json").unlink()
        self.assertIn("fixture missing: rust/symbrain-catalog/tests/fixtures/oracle_expectations.json", self.failures())

    def test_empty_fixture_is_rejected(self) -> None:
        (self.tmp / "rust/symbrain-mcp/tests/fixtures/oracle_expectations.json").write_bytes(b"")
        self.assertIn("fixture is empty", self.failures())

    def test_unclaimed_new_fixture_is_rejected(self) -> None:
        extra = self.tmp / "rust/symbrain-policy/tests/fixtures/new_oracle.json"
        extra.write_text("{}\n")
        self.assertIn("not claimed by any registry group", self.failures())

    def test_manifest_without_provenance_is_rejected(self) -> None:
        path = self.tmp / "migration/evidence/oracle-freeze-781/manifest.json"
        manifest = json.loads(path.read_text())
        del manifest["groups"][0]["provenance"]["go_inputs"]
        path.write_text(json.dumps(manifest))
        self.assertIn("provenance field 'go_inputs'", self.failures())

    def test_forged_hash_in_manifest_is_rejected(self) -> None:
        path = self.tmp / "migration/evidence/oracle-freeze-781/manifest.json"
        manifest = json.loads(path.read_text())
        manifest["groups"][1]["fixtures"][0]["sha256"] = "0" * 64
        path.write_text(json.dumps(manifest))
        self.assertIn("sha256 mismatch", self.failures())

    def test_registry_group_missing_from_manifest_is_rejected(self) -> None:
        path = self.tmp / "migration/evidence/oracle-freeze-781/manifest.json"
        manifest = json.loads(path.read_text())
        dropped = manifest["groups"].pop()
        path.write_text(json.dumps(manifest))
        self.assertIn(f"{dropped['id']}: registered but not sealed", self.failures())

    def test_memory_fixtures_are_outside_the_gate(self) -> None:
        extra = self.tmp / "rust/symbrain-memory/tests/fixtures/owned_elsewhere.json"
        extra.parent.mkdir(parents=True, exist_ok=True)
        extra.write_text("{}\n")
        freeze.verify(self.tmp)


if __name__ == "__main__":
    unittest.main()
