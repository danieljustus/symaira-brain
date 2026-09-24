"""The candidate selector must preserve a real Go rollback path."""
from __future__ import annotations

import json
import io
import os
import subprocess
import sys
import tarfile
import tempfile
import unittest
from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import launch_candidate
import verify


def write_archive(path: Path, payload: bytes) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for name, contents, mode in (
            ("symbrowse", payload, 0o755),
            ("LICENSE", b"license", 0o644),
            ("README.md", b"readme", 0o644),
            ("AGENTS.md", b"agents", 0o644),
        ):
            info = tarfile.TarInfo(name)
            info.size = len(contents)
            info.mode = mode
            archive.addfile(info, io.BytesIO(contents))


class LauncherTests(unittest.TestCase):
    def test_selection_fallback_and_integrity(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            dual = Path(raw) / "dual"
            dual.mkdir()
            (dual / "dual-release-manifest.json").write_text(
                json.dumps({"cutover_enabled": False, "selection_contract": verify.selection_manifest()}),
                encoding="utf-8",
            )
            name = verify.archive_name("1.2.3", "linux", "amd64")
            archives = {}
            for implementation in verify.IMPLEMENTATIONS:
                directory = dual / implementation
                directory.mkdir()
                archive = directory / name
                archive.write_bytes(implementation.encode())
                (directory / "checksums.txt").write_text(f"{verify._sha256(archive)}  {name}\n", encoding="utf-8")
                archives[implementation] = archive

            select = lambda requested: launch_candidate.select_archive(Path(raw), "1.2.3", "linux-amd64", requested)
            self.assertEqual(select(None)[0], "go")
            self.assertEqual(select("go")[0], "go")
            self.assertEqual(select("rust")[0], "rust")
            archives["rust"].unlink()
            self.assertEqual(select("rust")[0], "go")
            archives["rust"].write_bytes(b"tampered")
            with self.assertRaisesRegex(verify.GateError, "integrity"):
                select("rust")
            with self.assertRaisesRegex(verify.GateError, "exactly go or rust"):
                select("auto")
            with self.assertRaisesRegex(verify.GateError, "exactly go or rust"):
                select("")
            archives["go"].unlink()
            with self.assertRaisesRegex(verify.GateError, "unavailable"):
                select("go")

    def test_cli_runs_selected_implementation_and_go_rollback(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            dual = root / "dual"
            dual.mkdir()
            (dual / "dual-release-manifest.json").write_text(
                json.dumps({"cutover_enabled": False, "selection_contract": verify.selection_manifest()}),
                encoding="utf-8",
            )
            name = verify.archive_name("1.2.3", "linux", "amd64")
            go_dir = dual / "go"
            rust_dir = dual / "rust"
            go_dir.mkdir()
            rust_dir.mkdir()
            go_archive = go_dir / name
            write_archive(go_archive, b"go-known-good")
            (go_dir / "checksums.txt").write_text(
                f"{verify._sha256(go_archive)}  {name}\n", encoding="utf-8"
            )
            rust_archive = rust_dir / name
            write_archive(rust_archive, b"rust-candidate")
            (rust_dir / "checksums.txt").write_text(
                f"{verify._sha256(rust_archive)}  {name}\n", encoding="utf-8"
            )

            for requested, expected in ((None, b"go-known-good"), ("go", b"go-known-good"), ("rust", b"rust-candidate")):
                with self.subTest(requested=requested):
                    observed: list[bytes] = []

                    def run(command: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
                        self.assertEqual(command[1:], ["version", "--json"])
                        observed.append(Path(command[0]).read_bytes())
                        return subprocess.CompletedProcess(command, 23, "", "")

                    environment = {} if requested is None else {"SYMBROWSE_IMPL": requested}
                    argv = [
                        "launch_candidate.py", "--candidate", str(root), "--version", "1.2.3",
                        "--target", "linux-amd64", "--", "version", "--json",
                    ]
                    with patch.dict(os.environ, environment, clear=True), patch.object(sys, "argv", argv), patch.object(
                        launch_candidate.subprocess, "run", side_effect=run
                    ):
                        self.assertEqual(launch_candidate.main(), 23)
                    self.assertEqual(observed, [expected])

            rust_archive.unlink()
            observed = []
            argv = [
                "launch_candidate.py", "--candidate", str(root), "--version", "1.2.3",
                "--target", "linux-amd64", "--", "version", "--json",
            ]

            def run_go(command: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
                self.assertEqual(command[1:], ["version", "--json"])
                observed.append(Path(command[0]).read_bytes())
                return subprocess.CompletedProcess(command, 23, "", "")

            with patch.dict(os.environ, {"SYMBROWSE_IMPL": "rust"}, clear=True), patch.object(sys, "argv", argv), patch.object(
                launch_candidate.subprocess, "run", side_effect=run_go
            ):
                self.assertEqual(launch_candidate.main(), 23)
            self.assertEqual(observed, [b"go-known-good"])

    def test_cli_integrity_failure_never_executes_go_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            dual = root / "dual"
            dual.mkdir()
            (dual / "dual-release-manifest.json").write_text(
                json.dumps({"cutover_enabled": False, "selection_contract": verify.selection_manifest()}),
                encoding="utf-8",
            )
            name = verify.archive_name("1.2.3", "linux", "amd64")
            for implementation, payload in (("go", b"go-known-good"), ("rust", b"tampered-rust")):
                directory = dual / implementation
                directory.mkdir()
                archive = directory / name
                write_archive(archive, payload)
                checksum = verify._sha256(archive)
                if implementation == "rust":
                    archive.write_bytes(b"tampered after checksum")
                (directory / "checksums.txt").write_text(f"{checksum}  {name}\n", encoding="utf-8")

            argv = ["launch_candidate.py", "--candidate", str(root), "--version", "1.2.3", "--target", "linux-amd64"]
            with patch.dict(os.environ, {"SYMBROWSE_IMPL": "rust"}, clear=True), patch.object(sys, "argv", argv), patch.object(
                launch_candidate.subprocess, "run"
            ) as run:
                error = StringIO()
                with redirect_stderr(error), self.assertRaises(SystemExit) as raised:
                    launch_candidate.main()
            self.assertEqual(raised.exception.code, 1)
            self.assertIn("integrity failure", error.getvalue())
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
