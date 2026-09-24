#!/usr/bin/env python3
"""Focused checks for the unsigned six-target candidate merger."""
from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
import zipfile


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

    def test_merge_rejects_nested_symlink_before_reading_candidate_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            packages = root / "packages"
            packages.mkdir()
            source = subprocess.run(
                ["git", "rev-parse", "HEAD"], cwd=candidate.build_dual.ROOT, capture_output=True, text=True, check=True
            ).stdout.strip()
            target_dirs = [
                packages / f"symbrowse-dual-{source}-{os_name}-{arch}"
                for os_name, arch in candidate.verify.TARGETS
            ]
            for directory in target_dirs:
                directory.mkdir()
            outside = root / "outside.json"
            outside.write_text("{}", encoding="utf-8")
            try:
                (target_dirs[0] / "build-report.json").symlink_to(outside)
            except OSError as error:
                self.skipTest(f"symlinks unavailable in this runner: {error}")

            with self.assertRaisesRegex(candidate.verify.GateError, "candidate tree contains a symlink"):
                candidate.merge(packages, root / "output", "1.2.3", source)
            self.assertFalse((root / "output").exists())

    def test_archive_layout_keeps_paths_and_rejects_nested_or_duplicate_members(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / "candidate.zip"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("bin/symbrowse", b"binary")
            with self.assertRaisesRegex(candidate.verify.GateError, "archive layout mismatch"):
                candidate.verify._verify_archive_layout(
                    archive, {"symbrowse", "LICENSE", "README.md", "AGENTS.md"}
                )

    def test_candidate_spdx_binds_package_checksum_to_archive_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = root / "symbrowse_1.2.3_linux_amd64.tar.gz"
            archive.write_bytes(b"archive bytes")
            sbom = root / f"{archive.name}.sbom"
            candidate.build_dual.write_spdx(sbom, archive, archive.name, "rust", "1.2.3")
            candidate._verify_candidate_spdx(sbom, archive, "rust", "1.2.3")
            archive.write_bytes(b"changed archive bytes")
            with self.assertRaisesRegex(candidate.verify.GateError, "SPDX document identity mismatch"):
                candidate._verify_candidate_spdx(sbom, archive, "rust", "1.2.3")


if __name__ == "__main__":
    unittest.main()
