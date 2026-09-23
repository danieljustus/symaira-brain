#!/usr/bin/env python3
"""Small negative controls for release-verifier trust boundaries."""

import importlib.util
import io
import stat
import sys
import unittest
import zipfile
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("verify-release.py")
sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("verify_release", MODULE_PATH)
verify_release = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(verify_release)


class ReleaseVerifierTests(unittest.TestCase):
    def test_zip_symlink_is_rejected(self) -> None:
        archive_bytes = io.BytesIO()
        with zipfile.ZipFile(archive_bytes, "w") as archive:
            for name in ("AGENTS.md", "LICENSE", "README.md"):
                info = zipfile.ZipInfo(name)
                info.external_attr = (stat.S_IFREG | 0o644) << 16
                archive.writestr(info, "data")
            link = zipfile.ZipInfo("symbrain.exe")
            link.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(link, "target")
        with zipfile.ZipFile(archive_bytes) as archive:
            with self.assertRaisesRegex(SystemExit, "non-regular ZIP entry"):
                verify_release.zip_file_names(archive, "symbrain.zip")

    def test_unmanifested_asset_is_rejected(self) -> None:
        with self.assertRaisesRegex(SystemExit, "asset directory mismatch"):
            verify_release.ensure_exact_asset_names(["checksums.txt", "extra.txt"], {"checksums.txt"})

    def test_formula_comment_and_inactive_branch_are_not_active_links(self) -> None:
        scope = ("class Symbrain < Formula", "on_macos do", "if Hardware::CPU.intel?")
        source = '''class Symbrain < Formula
  on_macos do
    if Hardware::CPU.arm?
      url "https://example.invalid/release.tar.gz"
      sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    end
  end
end
# url "https://example.invalid/release.tar.gz"
# sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
'''
        self.assertEqual([], verify_release.ruby_url_checksum_pairs(source, {scope}))

    def test_cask_values_inside_inactive_block_are_not_active(self) -> None:
        source = '''cask "symbrain" do
  if false
    version "0.11.0"
    sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    url "https://example.invalid/app.dmg"
  end
end
'''
        root_scope = ("cask \"symbrain\" do",)
        active = [(key, value) for scope, key, value in verify_release.ruby_assignments(source) if scope == root_scope]
        self.assertEqual([], active)


if __name__ == "__main__":
    unittest.main()
