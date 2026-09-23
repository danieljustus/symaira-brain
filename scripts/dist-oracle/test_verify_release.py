#!/usr/bin/env python3
"""Small negative controls for release-verifier trust boundaries."""

import importlib.util
import hashlib
import io
import json
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

    def test_wrong_manifest_formula_digest_is_rejected(self) -> None:
        manifest = json.loads(verify_release.DEFAULT_MANIFEST.read_text())
        assets = {asset["name"]: asset for asset in manifest["assets"]}
        formula = "\n".join(
            f'url "https://{manifest["repository"]}/releases/download/v{manifest["version"]}/{name}"\n'
            f'sha256 "{assets[name]["digest"].removeprefix("sha256:")}"'
            for name in sorted(assets)
            if name.endswith((".tar.gz", ".zip")) and "_windows_" not in name
        )
        dmg_name = f'Symaira-Brain-{manifest["version"]}-macos.dmg'
        cask = (
            f'cask "symbrain" do\n  version "{manifest["version"]}"\n'
            f'  sha256 "{assets[dmg_name]["digest"].removeprefix("sha256:")}"\n'
            f'  url "https://{manifest["repository"]}/releases/download/v#{{version}}/Symaira-Brain-#{{version}}-macos.dmg"\nend\n'
        )
        formula_asset = next(name for name in assets if name.endswith("darwin_arm64.tar.gz"))
        assets[formula_asset]["digest"] = "sha256:" + "0" * 64
        with self.assertRaisesRegex(SystemExit, "Homebrew formula URLs/checksums differ"):
            verify_release.verify_tap_links(formula, cask, manifest)


if __name__ == "__main__":
    unittest.main()
