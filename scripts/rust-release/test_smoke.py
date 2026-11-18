#!/usr/bin/env python3
"""Tests for smoke.py: the symbrain archive smoke test covers the bundled workers."""

from __future__ import annotations

import shutil
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import smoke  # noqa: E402

VERSION = "0.17.0"
FAKE = """#!/bin/sh
case "$1" in
  version) echo "{name} {version}" ;;
  help) echo "Usage: {name}" ;;
  *) echo "unknown flag" >&2; exit 2 ;;
esac
"""


class Smoke(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="rust-release-smoke-test-"))
        self.addCleanup(shutil.rmtree, self.tmp)

    def extracted(self, names: list[str], versions: dict[str, str] | None = None) -> Path:
        directory = self.tmp / "extracted"
        directory.mkdir()
        for name in names:
            path = directory / name
            path.write_text(FAKE.format(name=name, version=(versions or {}).get(name, VERSION)))
            path.chmod(path.stat().st_mode | stat.S_IXUSR)
        return directory

    def smoke(self, directory: Path, platform: str, product: str = "symbrain") -> int:
        with mock.patch.object(smoke.sys, "platform", platform):
            return smoke.main(["--dir", str(directory), "--version", VERSION, "--product", product])

    def test_darwin_symbrain_archive_smokes_every_bundled_worker(self):
        directory = self.extracted(["symbrain", "symbrowse", "symoperate", "symscope"])
        self.assertEqual(self.smoke(directory, "darwin"), 0)

    def test_linux_symbrain_archive_needs_only_symbrowse(self):
        self.assertEqual(self.smoke(self.extracted(["symbrain", "symbrowse"]), "linux"), 0)

    def test_missing_bundled_worker_fails(self):
        for platform, names, missing in [("linux", ["symbrain"], "symbrowse"),
                                         ("darwin", ["symbrain", "symbrowse", "symoperate"], "symscope")]:
            with self.subTest(platform=platform, missing=missing):
                directory = self.extracted(names)
                with self.assertRaisesRegex(SystemExit, f"missing executable {missing}"):
                    self.smoke(directory, platform)
                shutil.rmtree(directory)

    def test_symbrowse_must_report_the_release_version(self):
        directory = self.extracted(["symbrain", "symbrowse"], {"symbrowse": "9.9.9"})
        with self.assertRaisesRegex(SystemExit, "symbrowse version failed or did not report"):
            self.smoke(directory, "linux")

    def test_worker_whose_version_command_fails_is_rejected(self):
        directory = self.extracted(["symbrain", "symbrowse"])
        (directory / "symbrowse").write_text("#!/bin/sh\nexit 3\n")
        with self.assertRaisesRegex(SystemExit, "symbrowse version"):
            self.smoke(directory, "linux")

    def test_standalone_symbrowse_archive_is_not_asked_for_other_workers(self):
        self.assertEqual(self.smoke(self.extracted(["symbrowse"]), "darwin", product="symbrowse"), 0)


if __name__ == "__main__":
    unittest.main()
