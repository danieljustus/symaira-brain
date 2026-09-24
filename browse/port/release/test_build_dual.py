#!/usr/bin/env python3
"""Focused tests for the dual-release Cargo target path."""
from __future__ import annotations

import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("browse_release_build_dual", ROOT / "browse/port/release/build_dual.py")
assert SPEC and SPEC.loader
build_dual = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = build_dual
SPEC.loader.exec_module(build_dual)


class CargoTargetDirTests(unittest.TestCase):
    def test_direct_macos_default_target_is_external(self) -> None:
        root = Path("/workspace/browse")
        with tempfile.TemporaryDirectory() as temp:
            runtime = Path(temp)
            env = {build_dual.EXTERNAL_BASE_ENV: str(runtime / "builds" / "browse")}
            with patch.object(build_dual, "EXTERNAL_RUNTIME_ROOT", runtime), patch.object(
                build_dual.Path, "is_mount", return_value=True, create=True
            ), patch.object(build_dual.sys, "platform", "darwin"), patch.dict(os.environ, {"CI": ""}, clear=False):
                target = build_dual._cargo_target_dir(root, env)
            self.assertEqual(target, Path(env[build_dual.EXTERNAL_BASE_ENV]) / "cargo-target")
            self.assertTrue(target.is_relative_to(runtime.resolve()))
            self.assertTrue(Path(env["GOTELEMETRYDIR"]).is_relative_to(runtime.resolve()))

    def test_direct_macos_output_rejects_local_absolute_path(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            runtime = Path(temp)
            env = {build_dual.EXTERNAL_BASE_ENV: str(runtime / "builds" / "browse")}
            with patch.object(build_dual, "EXTERNAL_RUNTIME_ROOT", runtime), patch.object(
                build_dual.Path, "is_mount", return_value=True, create=True
            ), patch.object(build_dual.sys, "platform", "darwin"), patch.dict(os.environ, {"CI": ""}, clear=False):
                build_dual.external_environment(env)
                with self.assertRaisesRegex(RuntimeError, "--output must be under"):
                    build_dual.release_output(Path("/workspace/browse"), Path("/tmp/release"), env)

    def test_ci_output_remains_portable(self) -> None:
        root = Path("/workspace/browse")
        self.assertEqual(
            build_dual.release_output(root, Path("target/release"), {"CI": "1"}),
            (root / "target/release").resolve(),
        )

    def test_ci_default_target_stays_in_browse_root(self) -> None:
        root = Path("/workspace/browse")
        with patch.object(build_dual.sys, "platform", "darwin"), patch.dict(os.environ, {"CI": "1"}, clear=False):
            self.assertEqual(build_dual._cargo_target_dir(root, {"CI": "1"}), root / "target")

    def test_relative_target_is_resolved_against_browse_root(self) -> None:
        root = Path("/workspace/browse")
        with patch.object(build_dual.sys, "platform", "linux"):
            self.assertEqual(
                build_dual._cargo_target_dir(root, {"CARGO_TARGET_DIR": "build/cargo-target"}),
                root / "build/cargo-target",
            )

    def test_absolute_target_is_preserved(self) -> None:
        root = Path("/workspace/browse")
        target = Path("/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/builds/browse-target")
        self.assertEqual(build_dual._cargo_target_dir(root, {"CARGO_TARGET_DIR": str(target)}), target)

    def test_existing_output_is_preserved_and_refused(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "candidate"
            output.mkdir()
            sentinel = output / "keep.txt"
            sentinel.write_text("preserve", encoding="utf-8")
            with patch.object(build_dual, "external_environment", side_effect=lambda env: env), patch.object(
                build_dual, "release_output", return_value=output
            ):
                with patch.object(sys, "argv", ["build_dual.py", "--source-revision", "a" * 40]):
                    with self.assertRaisesRegex(RuntimeError, "refusing to overwrite"):
                        build_dual.main()
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")


if __name__ == "__main__":
    unittest.main()
