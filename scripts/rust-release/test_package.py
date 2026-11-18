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
        self.workers = {}
        for name in ["symbrowse", "symoperate", "symscope"]:
            self.workers[name] = self.tmp / f"{name}-bin"
            self.workers[name].write_bytes(name.encode())

    def bundled(self, goos: str) -> list[str]:
        """The `--extra-binary` arguments every symbrain archive of this OS carries."""
        names = ["symbrowse", "symoperate", "symscope"] if goos == "darwin" else ["symbrowse"]
        return [arg for name in names for arg in ("--extra-binary", f"{name}={self.workers[name]}")]

    def build(self, out: Path, goos: str, goarch: str, epoch: str = "1700000000",
              product: str = "symbrain", extra: list[str] | None = None) -> Path:
        os.environ["SOURCE_DATE_EPOCH"] = epoch
        self.addCleanup(os.environ.pop, "SOURCE_DATE_EPOCH", None)
        args = ["archive", "--version", VERSION, "--os", goos, "--arch", goarch,
                "--rust", str(self.rust), "--out", str(out)]
        if product != "symbrain":  # the default product must keep working without the flag
            args += ["--product", product]
        elif extra is None:
            extra = self.bundled(goos)
        package.main(args + (extra or []))
        return out / package.archive_name(VERSION, goos, goarch, product)

    def build_bundle(self, out: Path) -> None:
        for product in package.PRODUCTS:
            for goos, goarch in package.PLATFORMS:
                self.build(out, goos, goarch, product=product)

    def test_tarball_members_modes_and_reproducibility(self):
        first, second = self.build(self.tmp / "a", "linux", "arm64"), self.build(self.tmp / "b", "linux", "arm64")
        self.assertEqual(first.name, f"symbrain_{VERSION}_linux_arm64.tar.gz")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with tarfile.open(first) as archive:
            info = {m.name: m for m in archive.getmembers()}
        self.assertEqual(sorted(info), ["AGENTS.md", "LICENSE", "README.md", "symbrain", "symbrowse"])
        self.assertEqual({info[name].mode for name in ["symbrain", "symbrowse"]}, {0o755})
        self.assertEqual(info["README.md"].mode, 0o644)
        self.assertEqual({(m.uid, m.gid, m.mtime) for m in info.values()}, {(0, 0, 1700000000)})

    def test_symbrowse_tarball_carries_its_own_binary(self):
        first = self.build(self.tmp / "a", "darwin", "arm64", product="symbrowse")
        second = self.build(self.tmp / "b", "darwin", "arm64", product="symbrowse")
        self.assertEqual(first.name, f"symbrowse_{VERSION}_darwin_arm64.tar.gz")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with tarfile.open(first) as archive:
            info = {m.name: m for m in archive.getmembers()}
        self.assertEqual(sorted(info), ["AGENTS.md", "LICENSE", "README.md", "symbrowse"])
        self.assertEqual(info["symbrowse"].mode, 0o755)

    def members_of(self, archive: Path) -> dict[str, tuple[int, bytes]]:
        with tarfile.open(archive) as tar:
            return {m.name: (m.mode, tar.extractfile(m).read()) for m in tar.getmembers()}

    def test_symbrain_archive_bundles_the_workers_per_platform(self):
        linux = self.members_of(self.build(self.tmp / "a", "linux", "amd64"))
        self.assertEqual(sorted(linux), ["AGENTS.md", "LICENSE", "README.md", "symbrain", "symbrowse"])
        darwin = self.members_of(self.build(self.tmp / "b", "darwin", "arm64"))
        self.assertEqual(sorted(darwin), ["AGENTS.md", "LICENSE", "README.md",
                                          "symbrain", "symbrowse", "symoperate", "symscope"])
        for name in ["symbrain", "symbrowse", "symoperate", "symscope"]:
            self.assertEqual(darwin[name][0], 0o755, name)
        self.assertEqual(darwin["symoperate"][1], b"symoperate")
        self.assertEqual(darwin["symbrain"][1], b"rust")

    def test_bundled_archive_is_reproducible_regardless_of_argument_order(self):
        names = ["symscope", "symbrowse", "symoperate"]
        reversed_args = [arg for name in names for arg in ("--extra-binary", f"{name}={self.workers[name]}")]
        first = self.build(self.tmp / "a", "darwin", "arm64")
        second = self.build(self.tmp / "b", "darwin", "arm64", extra=reversed_args)
        self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_extra_binary_rejects_unknown_misplaced_duplicate_and_malformed_workers(self):
        good = f"symbrowse={self.workers['symbrowse']}"
        cases = {
            "unknown worker": ["--extra-binary", f"symbrew={self.rust}"],
            "macOS-only worker on linux": ["--extra-binary", f"symoperate={self.workers['symoperate']}"],
            "duplicate": ["--extra-binary", good, "--extra-binary", good],
            "no name": ["--extra-binary", str(self.rust)],
            "missing file": ["--extra-binary", f"symbrowse={self.tmp / 'absent'}"],
            "empty file": ["--extra-binary", f"symbrowse={self.tmp / 'empty'}"],
        }
        (self.tmp / "empty").write_bytes(b"")
        for label, extra in cases.items():
            with self.subTest(label), self.assertRaises(SystemExit):
                self.build(self.tmp / "bad", "linux", "amd64", extra=extra)

    def test_standalone_products_take_no_extra_binary(self):
        with self.assertRaises(SystemExit):
            self.build(self.tmp / "bad", "linux", "amd64", product="symbrowse",
                       extra=["--extra-binary", f"symbrowse={self.workers['symbrowse']}"])

    def test_contract_rejects_symbrain_archives_missing_a_bundled_worker(self):
        for goos, goarch, missing in [("linux", "amd64", "symbrowse"), ("darwin", "arm64", "symscope"),
                                      ("darwin", "arm64", "symoperate")]:
            with self.subTest(platform=(goos, goarch), missing=missing):
                out = self.tmp / f"bundle-{goos}-{missing}"
                self.build_bundle(out)
                name = package.archive_name(VERSION, goos, goarch)
                extras = {n: p for n, p in self.workers.items() if n != missing and (goos == "darwin" or n == "symbrowse")}
                package.write_archive(out / name, package.members(self.rust, extras=extras), 1700000000)
                package.write_checksums(out)
                self.assertTrue(any(p.startswith(f"{name}: members") for p in package.verify(VERSION, out)))

    def test_contract_rejects_macos_workers_in_a_linux_archive(self):
        out = self.tmp / "bundle"
        self.build_bundle(out)
        name = package.archive_name(VERSION, "linux", "amd64")
        entries = package.members(self.rust, extras=self.workers)
        package.write_archive(out / name, entries, 1700000000)
        package.write_checksums(out)
        self.assertTrue(any(p.startswith(f"{name}: members") for p in package.verify(VERSION, out)))

    def test_contract_rejects_an_empty_or_unexecutable_bundled_worker(self):
        out = self.tmp / "bundle"
        self.build_bundle(out)
        name = package.archive_name(VERSION, "darwin", "arm64")
        entries = [(n, b"" if n == "symscope" else d, 0o644 if n == "symoperate" else m)
                   for n, d, m in package.members(self.rust, extras=self.workers)]
        package.write_archive(out / name, entries, 1700000000)
        package.write_checksums(out)
        problems = package.verify(VERSION, out)
        self.assertIn(f"{name}: empty symscope binary", problems)
        self.assertIn(f"{name}: symoperate is not a root-owned 0o755 file", problems)

    def test_symbrain_archive_is_unchanged_by_the_product_parameter(self):
        implicit = self.build(self.tmp / "a", "linux", "amd64")
        explicit = self.tmp / "b"
        package.main(["archive", "--version", VERSION, "--os", "linux", "--arch", "amd64",
                      "--product", "symbrain", "--rust", str(self.rust), "--out", str(explicit),
                      *self.bundled("linux")])
        self.assertEqual(implicit.read_bytes(), (explicit / implicit.name).read_bytes())

    def test_unknown_product_cannot_be_packaged(self):
        with self.assertRaises(SystemExit):
            self.build(self.tmp / "unknown", "linux", "amd64", product="symbrew")

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
        self.build_bundle(out)
        lines = package.write_checksums(out).read_text().splitlines()
        self.assertEqual(len(lines), 6)
        self.assertEqual({line.split("  ")[1].split("_")[0] for line in lines}, set(package.PRODUCTS))
        for line in lines:
            digest, name = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((out / name).read_bytes()).hexdigest())

    def test_full_bundle_passes_the_release_contract_check(self):
        out = self.tmp / "bundle"
        self.build_bundle(out)
        package.write_checksums(out)
        self.assertEqual(package.verify(VERSION, out), [])
        self.assertEqual(package.main(["verify", "--version", VERSION, "--dir", str(out)]), 0)

    def test_contract_rejects_a_bundle_without_symbrowse(self):
        out = self.tmp / "bundle"
        for goos, goarch in package.PLATFORMS:
            self.build(out, goos, goarch)
        package.write_checksums(out)
        problems = package.verify(VERSION, out)
        for goos, goarch in package.PLATFORMS:
            self.assertIn(f"missing archive {package.archive_name(VERSION, goos, goarch, 'symbrowse')}", problems)

    def test_contract_rejects_a_symbrowse_archive_holding_the_wrong_binary_name(self):
        out = self.tmp / "bundle"
        self.build_bundle(out)
        name = package.archive_name(VERSION, "linux", "arm64", "symbrowse")
        package.write_archive(out / name, package.members(self.rust, "symbrain"), 1700000000)
        package.write_checksums(out)
        self.assertTrue(any(p.startswith(f"{name}: members") for p in package.verify(VERSION, out)))

    def test_contract_rejects_missing_tampered_and_extra_archives(self):
        out = self.tmp / "bundle"
        self.build_bundle(out)
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
        self.build_bundle(out)
        name = package.archive_name(VERSION, "linux", "amd64")
        entries = package.members(self.rust, extras={"symbrowse": self.workers["symbrowse"]}) + [("symbrain-go", b"go", 0o755)]
        package.write_archive(out / name, sorted(entries), 1700000000)
        package.write_checksums(out)
        self.assertTrue(any("members" in problem for problem in package.verify(VERSION, out)))


if __name__ == "__main__":
    unittest.main()
