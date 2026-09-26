#!/usr/bin/env python3
"""Offline checks for the unsigned Homebrew candidate rehearsal."""
from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "browse_homebrew_rehearsal", Path(__file__).with_name("homebrew_rehearsal.py")
)
assert SPEC and SPEC.loader
homebrew_rehearsal = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = homebrew_rehearsal
SPEC.loader.exec_module(homebrew_rehearsal)


def write_archive(path: Path, payload: bytes) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for name, data, mode in (
            ("symbrowse", payload, 0o755),
            ("LICENSE", b"license", 0o644),
            ("README.md", b"readme", 0o644),
            ("AGENTS.md", b"agents", 0o644),
        ):
            entry = tarfile.TarInfo(name)
            entry.size = len(data)
            entry.mode = mode
            archive.addfile(entry, io.BytesIO(data))


class HomebrewRehearsalTests(unittest.TestCase):
    def test_temporary_formula_binds_candidate_file_and_digest(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            archive = Path(raw) / "symbrowse_1.2.3_darwin_arm64.tar.gz"
            archive.write_bytes(b"candidate")
            digest = hashlib.sha256(b"candidate").hexdigest()
            formula = homebrew_rehearsal.render_formula("1.2.3", archive, digest)
        self.assertIn(archive.resolve().as_uri(), formula)
        self.assertIn(f'sha256 "{digest}"', formula)
        self.assertIn('version "1.2.3"', formula)
        self.assertIn('bin.install "symbrowse"', formula)
        self.assertIn('"version", "--json"', formula)

    def test_missing_brew_is_a_typed_native_blocker(self) -> None:
        with patch.object(homebrew_rehearsal.shutil, "which", return_value=None):
            with self.assertRaisesRegex(homebrew_rehearsal.HomebrewBlocker, "HOMEBREW_UNAVAILABLE"):
                homebrew_rehearsal.style_formula(Path("symbrowse.rb"))

    @unittest.skipUnless(sys.platform == "darwin", "Homebrew Cellar links are macOS-native")
    def test_clean_prefix_failed_upgrade_then_rust_upgrade_and_go_rollback(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            dual = Path(raw) / "dual"
            (dual / "go").mkdir(parents=True)
            (dual / "rust").mkdir()
            (dual / "dual-release-manifest.json").write_text(
                json.dumps({"cutover_enabled": False, "selection_contract": {
                    "default": "go", "opt_in": "rust", "forced_go": "go",
                    "availability_failure": "fallback_go", "integrity_failure": "block_no_fallback",
                }}),
                encoding="utf-8",
            )
            version = "1.2.3"
            name = "symbrowse_1.2.3_darwin_arm64.tar.gz"
            for implementation, marker in (("go", b"go-binary"), ("rust", b"rust-binary")):
                archive = dual / implementation / name
                write_archive(archive, marker)
                (archive.parent / "checksums.txt").write_text(
                    f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {name}\n", encoding="utf-8"
                )

            def smoke(binary: Path, expected: str) -> dict[str, object]:
                if expected.endswith("deliberately-wrong"):
                    raise ValueError("candidate version identity mismatch")
                return {"tool": "symbrowse", "version": expected, "schema_version": 1}

            with patch.object(homebrew_rehearsal, "style_formula"), patch.object(
                homebrew_rehearsal.binary_smoke, "check_binary", side_effect=smoke
            ):
                result = homebrew_rehearsal.rehearse(Path(raw), version, "darwin-arm64")
        self.assertEqual(result["install"]["implementation"], "go")
        self.assertTrue(result["failed_upgrade"]["blocked"])
        self.assertEqual(result["upgrade"]["implementation"], "rust")
        self.assertEqual(result["forced_rollback"]["implementation"], "go")
        self.assertFalse(result["brew_install_or_tap_write"])


if __name__ == "__main__":
    unittest.main()
