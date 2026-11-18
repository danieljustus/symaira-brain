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
    def tap_examples(self) -> tuple[dict, str, str]:
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
        return manifest, formula, cask

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
        manifest, formula, cask = self.tap_examples()
        assets = {asset["name"]: asset for asset in manifest["assets"]}
        formula_asset = next(name for name in assets if name.endswith("darwin_arm64.tar.gz"))
        assets[formula_asset]["digest"] = "sha256:" + "0" * 64
        with self.assertRaisesRegex(SystemExit, "Homebrew formula URLs/checksums differ"):
            verify_release.verify_tap_links(formula, cask, manifest)

    def test_extra_formula_and_cask_release_urls_are_rejected(self) -> None:
        manifest, formula, cask = self.tap_examples()
        extra_formula = formula + '\nurl "https://example.invalid/releases/download/v0.10.0/rogue.tar.gz"\nsha256 "' + "a" * 64 + '"\n'
        with self.assertRaisesRegex(SystemExit, "formula has extra or missing release-download URLs"):
            verify_release.verify_tap_links(extra_formula, cask, manifest)
        extra_cask = cask + 'url "https://example.invalid/releases/download/v0.10.0/rogue.dmg"\n'
        with self.assertRaisesRegex(SystemExit, "cask has extra or missing release-download URLs"):
            verify_release.verify_tap_links(formula, extra_cask, manifest)

    def test_archive_binary_name_follows_the_product(self) -> None:
        cases = {
            "symbrain_0.15.0_darwin_arm64.tar.gz": "symbrain",
            "symbrowse_0.15.0_linux_amd64.tar.gz": "symbrowse",
            "symbrain_0.11.0_windows_amd64.zip": "symbrain.exe",
        }
        for name, binary in cases.items():
            with self.subTest(name=name):
                self.assertEqual(verify_release.archive_binary_name(name), binary)
        with self.assertRaisesRegex(SystemExit, "no known product"):
            verify_release.archive_binary_name("symbrew_0.15.0_linux_amd64.tar.gz")

    def test_symbrowse_archives_are_not_homebrew_formula_links(self) -> None:
        manifest, formula, cask = self.tap_examples()
        before = verify_release.verify_tap_links(formula, cask, manifest)
        version = manifest["version"]
        for goos, arch in (("darwin", "arm64"), ("linux", "amd64"), ("linux", "arm64")):
            manifest["assets"].append({
                "name": f"symbrowse_{version}_{goos}_{arch}.tar.gz",
                "digest": "sha256:" + "b" * 64,
                "size": 1,
            })
        self.assertEqual(verify_release.verify_tap_links(formula, cask, manifest), before)


    def test_bundled_symbrain_archives_hold_exactly_the_members_package_py_enforces(self) -> None:
        docs = ["AGENTS.md", "LICENSE", "README.md"]
        cases = {
            "symbrain_0.17.0_darwin_arm64.tar.gz": ["symbrain", "symbrowse", "symoperate", "symscope"],
            "symbrain_0.17.0_linux_amd64.tar.gz": ["symbrain", "symbrowse"],
            "symbrain_0.17.0_linux_arm64.tar.gz": ["symbrain", "symbrowse"],
            "symbrowse_0.17.0_darwin_arm64.tar.gz": ["symbrowse"],
            "symbrowse_0.17.0_linux_amd64.tar.gz": ["symbrowse"],
        }
        for name, binaries in cases.items():
            with self.subTest(name=name):
                verify_release.verify_archive_members(name, docs + binaries, bundled=True)

    def test_bundle_member_set_is_the_package_py_definition(self) -> None:
        package = verify_release.package
        for goos in ("darwin", "linux"):
            for product in ("symbrain", "symbrowse"):
                name = f"{product}_1.0.0_{goos}_arm64.tar.gz"
                self.assertEqual(verify_release.expected_archive_members(name, bundled=True),
                                 package.expected_members(product, goos))

    def test_partial_or_surplus_bundles_are_rejected(self) -> None:
        docs = ["AGENTS.md", "LICENSE", "README.md"]
        bad = {
            "symbrain_0.17.0_darwin_arm64.tar.gz": ["symbrain", "symbrowse", "symoperate"],
            "symbrain_0.17.0_darwin_arm64.tar.gz ": ["symbrain"],
            "symbrain_0.17.0_linux_amd64.tar.gz": ["symbrain"],
            "symbrain_0.17.0_linux_amd64.tar.gz  ": ["symbrain", "symbrowse", "symscope"],
            "symbrowse_0.17.0_linux_amd64.tar.gz": ["symbrowse", "symbrain"],
        }
        for name, binaries in bad.items():
            with self.subTest(name=name, binaries=binaries):
                with self.assertRaisesRegex(SystemExit, "unexpected archive contents"):
                    verify_release.verify_archive_members(name.strip(), docs + binaries, bundled=True)
        with self.assertRaisesRegex(SystemExit, "duplicate archive entries"):
            verify_release.verify_archive_members("symbrain_0.17.0_linux_amd64.tar.gz",
                                                  docs + ["symbrain", "symbrowse", "symbrowse"], bundled=True)

    def test_historical_single_binary_archives_still_verify_without_the_bundle_flag(self) -> None:
        docs = ["AGENTS.md", "LICENSE", "README.md"]
        verify_release.verify_archive_members("symbrain_0.11.0_darwin_arm64.tar.gz", docs + ["symbrain"], bundled=False)
        verify_release.verify_archive_members("symbrain_0.11.0_windows_amd64.zip", docs + ["symbrain.exe"], bundled=False)
        with self.assertRaisesRegex(SystemExit, "unexpected archive contents"):
            verify_release.verify_archive_members("symbrain_0.17.0_linux_amd64.tar.gz",
                                                  docs + ["symbrain", "symbrowse"], bundled=False)

    def test_bundle_mode_comes_from_the_manifest_or_the_flag(self) -> None:
        self.assertFalse(verify_release.is_bundled({}, False))
        self.assertTrue(verify_release.is_bundled({"bundled_workers": True}, False))
        self.assertTrue(verify_release.is_bundled({}, True))
        with self.assertRaisesRegex(SystemExit, "bundled_workers"):
            verify_release.is_bundled({"bundled_workers": "yes"}, False)


if __name__ == "__main__":
    unittest.main()
