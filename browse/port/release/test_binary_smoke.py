#!/usr/bin/env python3
"""Focused tests for the native dual-release binary smoke gate."""
from __future__ import annotations

import importlib.util
import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("browse_binary_smoke", Path(__file__).with_name("binary_smoke.py"))
assert SPEC and SPEC.loader
binary_smoke = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = binary_smoke
SPEC.loader.exec_module(binary_smoke)


class BinarySmokeTests(unittest.TestCase):
    def test_requires_exact_versionkit_identity(self) -> None:
        reply = subprocess.CompletedProcess(
            [], 0, '{"tool":"symbrowse","version":"1.2.3","schema_version":8}\n', ""
        )
        with patch.object(binary_smoke.subprocess, "run", return_value=reply) as run:
            self.assertEqual(binary_smoke.check_binary(Path("symbrowse"), "1.2.3"), {
                "tool": "symbrowse", "version": "1.2.3", "schema_version": 8
            })
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args_list[0].args[0], ["symbrowse", "version", "--json"])

    def test_rejects_wrong_binary_version(self) -> None:
        result = subprocess.CompletedProcess(
            [], 0, '{"tool":"symbrowse","version":"dev","schema_version":8}\n', ""
        )
        with patch.object(binary_smoke.subprocess, "run", return_value=result):
            with self.assertRaisesRegex(ValueError, "identity mismatch"):
                binary_smoke.check_binary(Path("symbrowse"), "1.2.3")


if __name__ == "__main__":
    unittest.main()
