#!/usr/bin/env python3
"""Meaningful negative controls for executable source identity, not timing thresholds."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("bench_identity_run", Path(__file__).with_name("run.py"))
RUN = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RUN
SPEC.loader.exec_module(RUN)


class IdentityTests(unittest.TestCase):
    def test_measurement_checkout_does_not_supply_binary_source(self):
        with tempfile.TemporaryDirectory() as raw:
            binary = Path(raw) / "fixture"
            binary.write_bytes(b"synthetic executable bytes")
            with patch.object(subprocess, "check_output", side_effect=AssertionError("no checkout inference")):
                identity = RUN.binary_identity(binary, Path(raw))
            self.assertEqual(identity["vcs_revision"], "unknown")

    def test_actual_go_metadata_and_unavailable_vcs_remain_distinct(self):
        with tempfile.TemporaryDirectory() as raw:
            binary = Path(raw) / "fixture"
            binary.write_bytes(b"synthetic executable bytes")
            for vcs in ("", "\tbuild\tvcs.revision=" + "a" * 40 + "\n\tbuild\tvcs.modified=false\n"):
                metadata = "fixture: go1.26.7\n\tbuild\tCGO_ENABLED=0\n" + vcs
                with patch.object(subprocess, "check_output", return_value=metadata):
                    identity = RUN.binary_identity(binary, Path(raw), go_binary=True)
                self.assertEqual(identity["vcs_revision"], "a" * 40 if vcs else "unknown")
                self.assertEqual(identity["go_build_metadata"], metadata)

    def test_missing_or_later_failed_measurement_cannot_be_reported_as_pass(self):
        self.assertEqual(RUN.summarize([])["status"], "error")
        result = RUN.summarize([dict(status="pass", duration_ns=10), dict(status="error", reason="actual process failed")])
        self.assertEqual(result["status"], "error")
        self.assertEqual(result["samples"][1]["reason"], "actual process failed")

    def test_receipt_for_another_executable_is_rejected(self):
        with tempfile.TemporaryDirectory() as raw:
            binary = Path(raw) / "fixture"
            binary.write_bytes(b"synthetic executable bytes")
            receipt = Path(raw) / "receipt.json"
            receipt.write_bytes(json.dumps(dict(binary_sha256="0" * 64, source_revision="a" * 40)).encode())
            with self.assertRaisesRegex(ValueError, "does not bind"):
                RUN.binary_identity(binary, Path(raw), build_receipt=receipt)


if __name__ == "__main__":
    unittest.main()
