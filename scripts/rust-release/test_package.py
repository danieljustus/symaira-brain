#!/usr/bin/env python3
"""Tests for package.py: reproducible archives and the release-contract check."""

from __future__ import annotations

import hashlib
import os
import shutil
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import package  # noqa: E402

VERSION = "0.12.1-candidate.0123456789ab"


class Package(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="rust-release-test-"))
        self.addCleanup(shutil.rmtree, self.tmp)
        self.rust = self.tmp / "rust-bin"
        self.rust.write_bytes(b"rust")

    def build(self, out: Path, goos: str, goarch: str, epoch: str = "1700000000") -> Path:
        os.environ["SOURCE_DATE_EPOCH"] = epoch
        self.addCleanup(os.environ.pop, "SOURCE_DATE_EPOCH", None)
        package.main(["archive", "--version", VERSION, "--os", goos, "--arch", goarch,
                      "--rust", str(self.rust), "--out", str(out)])
        return out / package.archive_name(VERSION, goos, goarch)

    def test_tarball_members_modes_and_reproducibility(self):
        first, second = self.build(self.tmp / "a", "linux", "arm64"), self.build(self.tmp / "b", "linux", "arm64")
        self.assertEqual(first.name, f"symbrain_{VERSION}_linux_arm64.tar.gz")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with tarfile.open(first) as archive:
            info = {m.name: m for m in archive.getmembers()}
        self.assertEqual(sorted(info), ["AGENTS.md", "LICENSE", "README.md", "symbrain"])
        self.assertEqual(info["symbrain"].mode, 0o755)
        self.assertEqual(info["README.md"].mode, 0o644)
        self.assertEqual({(m.uid, m.gid, m.mtime) for m in info.values()}, {(0, 0, 1700000000)})

    def test_removed_platforms_cannot_be_packaged(self):
        for goos, goarch in [("windows", "arm64"), ("windows", "amd64"), ("darwin", "amd64")]:
            with self.subTest(platform=(goos, goarch)), self.assertRaises(SystemExit):
                self.build(self.tmp / "unsupported", goos, goarch)

    def test_rejects_v_prefixed_version(self):
        with self.assertRaises(SystemExit):
            package.main(["archive", "--version", "v0.12.1", "--os", "linux", "--arch", "amd64",
                          "--rust", str(self.rust), "--out", str(self.tmp)])

    def test_checksums_cover_every_archive_in_sha256sum_format(self):
        out = self.tmp / "bundle"
        for goos, goarch in package.PLATFORMS:
            self.build(out, goos, goarch)
        lines = package.write_checksums(out).read_text().splitlines()
        self.assertEqual(len(lines), 3)
        for line in lines:
            digest, name = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((out / name).read_bytes()).hexdigest())

    def test_full_bundle_passes_the_release_contract_check(self):
        out = self.tmp / "bundle"
        for goos, goarch in package.PLATFORMS:
            self.build(out, goos, goarch)
        package.write_checksums(out)
        self.assertEqual(package.verify(VERSION, out), [])
        self.assertEqual(package.main(["verify", "--version", VERSION, "--dir", str(out)]), 0)

    def test_contract_rejects_missing_tampered_and_extra_archives(self):
        out = self.tmp / "bundle"
        for goos, goarch in package.PLATFORMS:
            self.build(out, goos, goarch)
        package.write_checksums(out)
        darwin = out / package.archive_name(VERSION, "darwin", "arm64")
        darwin.write_bytes(darwin.read_bytes() + b"x")
        self.assertIn(f"{darwin.name}: checksum mismatch", package.verify(VERSION, out))
        darwin.unlink()
        package.write_checksums(out)
        self.assertIn(f"missing archive {darwin.name}", package.verify(VERSION, out))
        self.assertEqual(package.main(["verify", "--version", VERSION, "--dir", str(out)]), 1)

    def test_contract_rejects_a_go_fallback_member(self):
        out = self.tmp / "bundle"
        for goos, goarch in package.PLATFORMS:
            self.build(out, goos, goarch)
        name = package.archive_name(VERSION, "linux", "amd64")
        entries = package.members(self.rust) + [("symbrain-go", b"go", 0o755)]
        package.write_archive(out / name, sorted(entries), 1700000000)
        package.write_checksums(out)
        self.assertTrue(any("members" in problem for problem in package.verify(VERSION, out)))


if __name__ == "__main__":
    unittest.main()
