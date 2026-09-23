#!/usr/bin/env python3
"""Small negative controls for release-verifier trust boundaries."""

import importlib.util
import hashlib
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

    def test_changed_tap_comments_and_dynamic_calls_break_pinned_bytes(self) -> None:
        canonical = b'url "https://example.invalid/app.dmg"\nsha256 "aaaaaaaa"\n'
        digest = "sha256:" + hashlib.sha256(canonical).hexdigest()
        replacements = (
            b"=begin\nurl \"https://example.invalid/app.dmg\"\n=end\n",
            b'url("https://example.invalid/app.dmg")\nsha256("aaaaaaaa")\n',
        )
        for changed in replacements:
            with self.subTest(changed=changed):
                with self.assertRaisesRegex(SystemExit, "differs from pinned live tap bytes"):
                    verify_release.require_pinned_bytes(changed, digest, "Homebrew source")


if __name__ == "__main__":
    unittest.main()
