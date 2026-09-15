#!/usr/bin/env python3
"""Unit tests and negative controls for init-oracle comparator."""
from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from typing import Any
import unittest
from unittest.mock import patch

from compare import (
    MANDATORY_GO_SOURCE_FILES,
    PINNED_COMMIT_SHA,
    REQUIRED_CASE_IDS,
    CaseRunner,
    CaseSpec,
    build_parser,
    build_go_reference_binary,
    capture_fs_manifest,
    extract_and_verify_go_sources,
    get_case_specs,
    get_git_tree_inventory,
    git_archive_tar,
    main,
    run_bounded,
    sha256_bytes,
    sha256_file,
    validate_archive_path_containment,
    validate_case_inventory,
    verify_autocrlf_hostility_resilience,
    verify_commit_sha,
)

REPO_ROOT = Path(__file__).resolve().parents[2]


def create_test_script(path_without_ext: Path, python_code: str) -> Path:
    """Creates a platform-correct executable test helper script (cmd wrapper on Windows, chmod +x on POSIX)."""
    if sys.platform == "win32" or os.name == "nt":
        py_path = path_without_ext.with_suffix(".py")
        py_path.write_text(python_code, encoding="utf-8")
        script_path = path_without_ext.with_suffix(".cmd")
        script_path.write_text(f'@"{sys.executable}" "{py_path.resolve()}" %*\n', encoding="utf-8")
        return script_path
    else:
        script_path = path_without_ext
        script_path.write_text(f"#!{sys.executable}\n{python_code}\n", encoding="utf-8")
        os.chmod(script_path, 0o755)
        return script_path


class TestInitOracle(unittest.TestCase):
    """Unit tests and negative controls for the compare.py routines."""

    def test_sha256_helpers(self) -> None:
        data = b"hello world\n"
        expected = hashlib.sha256(data).hexdigest()
        self.assertEqual(sha256_bytes(data), expected)

        with tempfile.NamedTemporaryFile(delete=False) as f:
            f.write(data)
            temp_path = Path(f.name)
        try:
            self.assertEqual(sha256_file(temp_path), expected)
        finally:
            temp_path.unlink()

    def test_verify_commit_sha(self) -> None:
        valid_sha = "d53c3824e7d6771fabefd62a18dd3c5e50c3c37d"
        self.assertEqual(verify_commit_sha(valid_sha), valid_sha)

        with self.assertRaises(ValueError):
            verify_commit_sha("short")
        with self.assertRaises(ValueError):
            verify_commit_sha("D53C3824E7D6771FABEFD62A18DD3C5E50C3C37D")
        with self.assertRaises(ValueError):
            verify_commit_sha("d53c3824e7d6771fabefd62a18dd3c5e50c3c37z")

    def test_capture_fs_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "dir1").mkdir()
            (root / "dir1" / "file1.txt").write_bytes(b"content1")
            os.chmod(root / "dir1" / "file1.txt", 0o600)
            os.chmod(root / "dir1", 0o700)

            symlink_created = False
            try:
                os.symlink("file1.txt", str(root / "dir1" / "symlink1"))
                symlink_created = True
            except OSError:
                if os.name == "posix":
                    raise

            manifest = capture_fs_manifest(root)
            paths = [m["path"] for m in manifest]

            self.assertIn("dir1", paths)
            self.assertIn("dir1/file1.txt", paths)
            if symlink_created:
                self.assertIn("dir1/symlink1", paths)
                sym_entry = next(m for m in manifest if m["path"] == "dir1/symlink1")
                self.assertEqual(sym_entry["type"], "symlink")
                self.assertEqual(sym_entry["target"], "file1.txt")

            file_entry = next(m for m in manifest if m["path"] == "dir1/file1.txt")
            self.assertEqual(file_entry["type"], "file")
            self.assertEqual(file_entry["size"], 8)
            self.assertEqual(file_entry["sha256"], sha256_bytes(b"content1"))

    def test_validate_archive_path_containment(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            base = Path(td)
            valid = validate_archive_path_containment(base, "cmd/symbrain/main.go")
            self.assertEqual(valid, (base / "cmd/symbrain/main.go").resolve())

            with self.assertRaises(RuntimeError):
                validate_archive_path_containment(base, "../escape.txt")

            with self.assertRaises(RuntimeError):
                validate_archive_path_containment(base, "a/../../escape.txt")

            with self.assertRaises(RuntimeError):
                validate_archive_path_containment(base, ".")

    def test_autocrlf_hostility_resilience_via_real_helper(self) -> None:
        """Verifies that the real git_archive_tar helper withstands hostile GIT_CONFIG_COUNT."""
        verify_autocrlf_hostility_resilience(REPO_ROOT, PINNED_COMMIT_SHA)

        # Direct test with hostile env on real git_archive_tar
        hostile_env = {
            "GIT_CONFIG_COUNT": "2",
            "GIT_CONFIG_KEY_0": "core.autocrlf",
            "GIT_CONFIG_VALUE_0": "true",
            "GIT_CONFIG_KEY_1": "core.eol",
            "GIT_CONFIG_VALUE_1": "crlf",
        }
        tar_bytes = git_archive_tar(
            REPO_ROOT,
            PINNED_COMMIT_SHA,
            paths=["cmd/symbrain/cmd_init.go"],
            extra_env=hostile_env,
        )
        with tempfile.NamedTemporaryFile(suffix=".tar", delete=False) as tf:
            tf.write(tar_bytes)
            tname = tf.name
        try:
            import tarfile
            with tarfile.open(tname, "r:") as tar:
                f = tar.extractfile("cmd/symbrain/cmd_init.go")
                self.assertIsNotNone(f)
                content = f.read()
                self.assertNotIn(b"\r\n", content)
        finally:
            os.unlink(tname)

        # Negative control: verify that omitting the explicit flags fails under hostile env
        raw_hostile_cmd = [
            "git",
            "archive",
            "--format=tar",
            PINNED_COMMIT_SHA,
            "cmd/symbrain/cmd_init.go",
        ]
        hostile_tar_bytes = subprocess.check_output(
            raw_hostile_cmd, cwd=REPO_ROOT, env={**os.environ, **hostile_env}
        )
        with tempfile.NamedTemporaryFile(suffix=".tar", delete=False) as tf:
            tf.write(hostile_tar_bytes)
            tname = tf.name
        try:
            with tarfile.open(tname, "r:") as tar:
                f = tar.extractfile("cmd/symbrain/cmd_init.go")
                self.assertIsNotNone(f)
                raw_content = f.read()
                self.assertIn(
                    b"\r\n",
                    raw_content,
                    "Negative control: unflagged git archive should have produced CRLF under hostile env",
                )
        finally:
            os.unlink(tname)

    def test_extract_and_verify_go_sources(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            dest = Path(td) / "go_sources"
            manifest_dict, source_manifest = extract_and_verify_go_sources(
                REPO_ROOT,
                PINNED_COMMIT_SHA,
                dest,
            )
            for mandatory in MANDATORY_GO_SOURCE_FILES:
                self.assertIn(mandatory, manifest_dict)
                self.assertTrue((dest / mandatory).is_file())

            tree_inv = get_git_tree_inventory(REPO_ROOT, PINNED_COMMIT_SHA)
            regular_blobs = [k for k, v in tree_inv.items() if v["type"] == "blob"]
            self.assertEqual(len(manifest_dict), len(regular_blobs))
            self.assertEqual(len(source_manifest), len(regular_blobs))

    def test_negative_control_corrupted_or_missing_source(self) -> None:
        """Negative control: missing or tampered source fails verification."""
        with tempfile.TemporaryDirectory() as td:
            dest = Path(td) / "go_sources"
            with self.assertRaises(ValueError):
                extract_and_verify_go_sources(REPO_ROOT, "invalid_sha", dest)

    def test_negative_control_stderr_mismatch_same_nonzero_exit(self) -> None:
        """Negative control: same exit code 1 with different stderr must fail parity."""
        with tempfile.TemporaryDirectory() as td:
            base = Path(td)
            bin_dir = base / "bin"
            bin_dir.mkdir(parents=True, exist_ok=True)

            go_script = create_test_script(
                bin_dir / "go_bin",
                "import sys\n"
                "sys.stderr.write('symbrain init: go specific failure\\n')\n"
                "sys.exit(1)\n",
            )
            rust_script = create_test_script(
                bin_dir / "rust_bin",
                "import sys\n"
                "sys.stderr.write('symbrain init: rust specific failure\\n')\n"
                "sys.exit(1)\n",
            )

            runner = CaseRunner(
                go_binary=go_script,
                rust_binary=rust_script,
                base_dir=base,
            )

            res = runner.run_case(
                "test_stderr_mismatch",
                "Negative control for stderr difference on nonzero exit",
                lambda p: None,
                [],
            )

            self.assertEqual(res["status"], "fail")
            self.assertTrue(any("stderr mismatch" in d for d in res["diffs"]))
            self.assertEqual(res["go"]["exit_code"], 1)
            self.assertEqual(res["rust"]["exit_code"], 1)
            self.assertIn("go specific failure", res["go"]["stderr"])
            self.assertIn("rust specific failure", res["rust"]["stderr"])
            self.assertIsNotNone(res["go"]["stderr_b64"])
            self.assertIsNotNone(res["rust"]["stderr_b64"])

    def test_negative_control_timeout_equality(self) -> None:
        """Negative control: matching timeout must fail parity comparison."""
        with tempfile.TemporaryDirectory() as td:
            base = Path(td)
            bin_dir = base / "bin"
            bin_dir.mkdir(parents=True, exist_ok=True)

            sleep_script = create_test_script(
                bin_dir / "slow_bin",
                "import time\n"
                "time.sleep(10)\n",
            )

            runner = CaseRunner(
                go_binary=sleep_script,
                rust_binary=sleep_script,
                base_dir=base,
                case_timeout=0.1,
            )

            res = runner.run_case(
                "test_timeout",
                "Negative control for timeout equality",
                lambda p: None,
                [],
            )

            self.assertEqual(res["status"], "fail")
            self.assertTrue(res["go"]["timed_out"])
            self.assertTrue(res["rust"]["timed_out"])
            self.assertTrue(any("timed out" in d for d in res["diffs"]))

    def test_negative_control_symlink_seed_and_cwd_handling(self) -> None:
        """Verifies that symlinks (dangling & cyclic) are preserved and cwd is recreated on capable systems."""
        with tempfile.TemporaryDirectory() as td:
            base = Path(td)
            bin_dir = base / "bin"
            bin_dir.mkdir(parents=True, exist_ok=True)

            script = create_test_script(
                bin_dir / "noop_bin",
                "import os, sys\n"
                "# Verify current working directory exists\n"
                "assert os.path.isdir(os.getcwd())\n"
                "sys.exit(0)\n",
            )

            symlinks_supported = False
            probe_link = base / "probe_symlink"
            try:
                os.symlink("probe_target", str(probe_link))
                symlinks_supported = True
                probe_link.unlink()
            except OSError:
                if os.name == "posix":
                    raise

            if not symlinks_supported:
                return

            def seed_with_symlinks(root: Path) -> None:
                (root / "dangling").symlink_to("nonexistent.target")
                (root / "cyclic").symlink_to("cyclic")

            runner = CaseRunner(
                go_binary=script,
                rust_binary=script,
                base_dir=base,
            )

            res = runner.run_case(
                "test_symlink_seed",
                "Symlink preservation and nested cwd recreation",
                seed_with_symlinks,
                [],
                working_sub_dir="nested/working/sub/dir",
            )

            self.assertEqual(res["status"], "pass")
            self.assertEqual(len(res["diffs"]), 0)
            go_paths = [m["path"] for m in res["go"]["manifest"]]
            self.assertIn("dangling", go_paths)
            self.assertIn("cyclic", go_paths)

    def test_case_inventory_validation(self) -> None:
        """Verifies inventory rules: empty, duplicates, missing required, unequal inventory."""
        declared = get_case_specs()

        # 1. Valid inventory passes
        valid_results = [
            {"case_id": s.case_id, "status": "pass", "diffs": []}
            for s in declared
        ]
        validate_case_inventory(declared, valid_results)

        # 2. Empty inventory fails
        with self.assertRaises(ValueError) as ctx:
            validate_case_inventory(declared, [])
        self.assertIn("empty", str(ctx.exception))

        # 3. Duplicate case ID fails
        dup_results = list(valid_results) + [valid_results[0]]
        with self.assertRaises(ValueError) as ctx:
            validate_case_inventory(declared, dup_results)
        self.assertIn("Duplicate", str(ctx.exception))

        # 4. Missing required case ID fails
        missing_req_results = [r for r in valid_results if r["case_id"] != "fresh"]
        with self.assertRaises(ValueError) as ctx:
            validate_case_inventory(declared, missing_req_results)
        self.assertIn("Missing required", str(ctx.exception))

        # 5. Unequal declared vs executed inventory fails
        permuted_results = list(reversed(valid_results))
        with self.assertRaises(ValueError) as ctx:
            validate_case_inventory(declared, permuted_results)
        self.assertIn("does not match", str(ctx.exception))

    def test_cli_flags_frozen_commit_and_no_go_binary(self) -> None:
        """Verifies CLI frozen commit and elimination of --go-binary via real compare.main/parser entrypoint."""
        import compare

        with tempfile.TemporaryDirectory() as td:
            dummy_rust = Path(td) / "dummy_rust"
            dummy_rust.write_bytes(b"dummy_binary")
            dummy_out = Path(td) / "report.json"

            def forbidden_execution(*args: Any, **kwargs: Any) -> Any:
                raise AssertionError(
                    "Forbidden source/build execution was reached; CLI guard was bypassed!"
                )

            with patch("compare.extract_and_verify_go_sources", side_effect=forbidden_execution), \
                 patch("compare.build_go_reference_binary", side_effect=forbidden_execution), \
                 patch("compare.run_all_cases", side_effect=forbidden_execution), \
                 patch("compare.verify_autocrlf_hostility_resilience", side_effect=forbidden_execution):

                # 1. Real compare.main entrypoint rejects invalid commit SHA
                with self.assertRaises(SystemExit):
                    compare.main([
                        "--rust-binary", str(dummy_rust),
                        "--output", str(dummy_out),
                        "--commit", "1111111111111111111111111111111111111111",
                    ])

                # 2. Real compare.main entrypoint rejects unpinned commit even if valid 40-hex
                with self.assertRaises(SystemExit):
                    compare.main([
                        "--rust-binary", str(dummy_rust),
                        "--output", str(dummy_out),
                        "--commit", "0000000000000000000000000000000000000000",
                    ])

                # 3. Real compare.main entrypoint rejects unaccepted --go-binary flag
                with self.assertRaises(SystemExit):
                    compare.main([
                        "--rust-binary", str(dummy_rust),
                        "--output", str(dummy_out),
                        "--go-binary", "/some/path/to/go",
                    ])

                # 4. Direct build_parser verification
                parser = compare.build_parser()
                parsed = parser.parse_args([
                    "--rust-binary", str(dummy_rust),
                    "--output", str(dummy_out),
                    "--commit", PINNED_COMMIT_SHA,
                ])
                self.assertEqual(parsed.commit, PINNED_COMMIT_SHA)

                # Wrong commit rejected by parser
                with self.assertRaises(SystemExit):
                    parser.parse_args([
                        "--rust-binary", str(dummy_rust),
                        "--output", str(dummy_out),
                        "--commit", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ])

                # --go-binary rejected by parser
                with self.assertRaises(SystemExit):
                    parser.parse_args([
                        "--rust-binary", str(dummy_rust),
                        "--output", str(dummy_out),
                        "--go-binary", "/some/path",
                    ])


class BuildFailureEvidence(unittest.TestCase):
    def test_failed_and_timed_out_builds_retain_raw_streams(self):
        for timed_out in (False, True):
            with self.subTest(timed_out=timed_out), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                version = subprocess.CompletedProcess(["go", "version"], 0, stdout="go version go1.26.7 test/test\n")
                failure = (subprocess.TimeoutExpired(["go", "build"], 180, output=b"raw stdout", stderr=b"raw stderr")
                           if timed_out else subprocess.CompletedProcess(["go", "build"], 23, stdout=b"raw stdout", stderr=b"raw stderr"))
                with patch("compare.subprocess.run", side_effect=[version, failure]):
                    with self.assertRaises(subprocess.TimeoutExpired if timed_out else subprocess.CalledProcessError):
                        build_go_reference_binary(root, root / "bin/oracle", root / "cache")
                self.assertEqual((root / "bin/build-stdout.log").read_bytes(), b"raw stdout")
                self.assertEqual((root / "bin/build-stderr.log").read_bytes(), b"raw stderr")
                self.assertFalse((root / "bin/oracle").exists())


if __name__ == "__main__":
    unittest.main()
