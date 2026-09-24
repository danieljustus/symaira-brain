"""The candidate selector must preserve a real Go rollback path."""
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import launch_candidate
import verify


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
            archives["go"].unlink()
            with self.assertRaisesRegex(verify.GateError, "unavailable"):
                select("go")


if __name__ == "__main__":
    unittest.main()
