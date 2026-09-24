#!/usr/bin/env python3
"""Focused checks for the unsigned six-target candidate merger."""
from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location("browse_release_candidate", Path(__file__).with_name("candidate.py"))
assert SPEC and SPEC.loader
candidate = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = candidate
SPEC.loader.exec_module(candidate)


class CandidateTests(unittest.TestCase):
    def test_runner_identity_must_match_each_native_target(self) -> None:
        self.assertTrue(candidate._runner_matches({"runner": "Linux/x86_64"}, "linux-amd64"))
        self.assertTrue(candidate._runner_matches({"runner": "Windows/ARM64"}, "windows-arm64"))
        self.assertFalse(candidate._runner_matches({"runner": "Linux/x86_64"}, "linux-arm64"))

    def test_merge_refuses_existing_output_without_removing_contents(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "candidate"
            output.mkdir()
            sentinel = output / "keep.txt"
            sentinel.write_text("preserve", encoding="utf-8")
            with self.assertRaisesRegex(candidate.verify.GateError, "refusing to overwrite"):
                candidate.merge(Path(temp) / "packages", output, "1.2.3", "a" * 40)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")

    def test_merge_rejects_incomplete_same_sha_matrix_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            packages = root / "packages"
            packages.mkdir()
            source = subprocess.run(
                ["git", "rev-parse", "HEAD"], cwd=candidate.build_dual.ROOT, capture_output=True, text=True, check=True
            ).stdout.strip()
            with self.assertRaisesRegex(candidate.verify.GateError, "exactly the six same-SHA"):
                candidate.merge(packages, root / "output", "1.2.3", source)
            self.assertFalse((root / "output").exists())


if __name__ == "__main__":
    unittest.main()
