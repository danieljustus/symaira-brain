#!/usr/bin/env python3
"""Tests for package.py; the bundle check uses the real dist-oracle when Go is available."""

from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import package  # noqa: E402

VERSION = "0.12.1-candidate.0123456789ab"


class Package(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="rust-release-test-"))
        self.addCleanup(shutil.rmtree, self.tmp)
        self.rust, self.go = self.tmp / "rust-bin", self.tmp / "go-bin"
        self.rust.write_bytes(b"rust")
        self.go.write_bytes(b"go")

    def build(self, out: Path, goos: str, goarch: str, epoch: str = "1700000000") -> Path:
        os.environ["SOURCE_DATE_EPOCH"] = epoch
        self.addCleanup(os.environ.pop, "SOURCE_DATE_EPOCH", None)
        package.main(["archive", "--version", VERSION, "--os", goos, "--arch", goarch,
                      "--rust", str(self.rust), "--go", str(self.go), "--out", str(out)])
        return out / package.archive_name(VERSION, goos, goarch)

    def test_tarball_members_modes_and_reproducibility(self):
        first, second = self.build(self.tmp / "a", "linux", "arm64"), self.build(self.tmp / "b", "linux", "arm64")
        self.assertEqual(first.name, f"symbrain_{VERSION}_linux_arm64.tar.gz")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with tarfile.open(first) as archive:
            info = {m.name: m for m in archive.getmembers()}
        self.assertEqual(sorted(info), ["AGENTS.md", "LICENSE", "README.md", "symbrain", "symbrain-go"])
        self.assertEqual(info["symbrain"].mode, 0o755)
        self.assertEqual(info["README.md"].mode, 0o644)
        self.assertEqual({(m.uid, m.gid, m.mtime) for m in info.values()}, {(0, 0, 1700000000)})

    def test_windows_zip_uses_exe_members(self):
        path = self.build(self.tmp / "w", "windows", "arm64")
        self.assertEqual(path.suffix, ".zip")
        with zipfile.ZipFile(path) as archive:
            self.assertEqual(sorted(archive.namelist()),
                             ["AGENTS.md", "LICENSE", "README.md", "symbrain-go.exe", "symbrain.exe"])
            self.assertEqual(archive.read("symbrain.exe"), b"rust")
        self.assertEqual(path.read_bytes(), self.build(self.tmp / "w2", "windows", "arm64").read_bytes())

    def test_rejects_v_prefixed_version(self):
        with self.assertRaises(SystemExit):
            package.main(["archive", "--version", "v0.12.1", "--os", "linux", "--arch", "amd64",
                          "--rust", str(self.rust), "--go", str(self.go), "--out", str(self.tmp)])

    def test_checksums_cover_every_archive_in_sha256sum_format(self):
        out = self.tmp / "bundle"
        for goos, goarch in package.PLATFORMS:
            self.build(out, goos, goarch)
        lines = package.write_checksums(out).read_text().splitlines()
        self.assertEqual(len(lines), 6)
        for line in lines:
            digest, name = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((out / name).read_bytes()).hexdigest())

    @unittest.skipUnless(shutil.which("go"), "dist-oracle needs Go")
    def test_full_bundle_passes_the_release_contract_check(self):
        out = self.tmp / "bundle"
        for goos, goarch in package.PLATFORMS:
            self.build(out, goos, goarch)
        package.write_checksums(out)
        check = ["go", "run", "./scripts/dist-oracle", "-hybrid-candidate-check", "-platforms", "all",
                 "-version", VERSION, "-assets", str(out)]
        result = subprocess.run(check, cwd=package.ROOT, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS 6 archives", result.stdout)
        (out / package.archive_name(VERSION, "darwin", "arm64")).unlink()
        package.write_checksums(out)
        self.assertNotEqual(subprocess.run(check, cwd=package.ROOT, capture_output=True).returncode, 0)


if __name__ == "__main__":
    unittest.main()
