#!/usr/bin/env python3
"""Small negative controls for release-verifier trust boundaries."""

import importlib.util
import hashlib
import io
import json
import stat
import sys
import tempfile
from unittest.mock import patch
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

    def test_signature_preflight_rejects_missing_or_malformed_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            payload_name = "checksums.txt"
            payload = root / payload_name
            payload.write_bytes(b"payload")
            files = {payload_name: payload}
            with self.assertRaisesRegex(SystemExit, "missing signature sidecar"):
                verify_release.require_signature_sidecars({payload_name}, files)

            signature = root / (payload_name + ".sig")
            certificate = root / (payload_name + ".pem")
            certificate.write_bytes(b"not a certificate")
            files[certificate.name] = certificate
            signature.write_bytes(b"")
            files[signature.name] = signature
            with self.assertRaisesRegex(SystemExit, "empty signature sidecar"):
                verify_release.require_signature_sidecars({payload_name}, files)

            signature.write_bytes(b"invalid synthetic signature fixture")
            files.pop(certificate.name)
            with self.assertRaisesRegex(SystemExit, "missing certificate sidecar"):
                verify_release.require_signature_sidecars({payload_name}, files)

            files[certificate.name] = certificate
            with self.assertRaisesRegex(SystemExit, "invalid certificate PEM metadata"):
                verify_release.require_signature_sidecars({payload_name}, files)

    def test_cosign_rejection_blocks_invalid_signature_claim(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            payload = root / "checksums.txt"
            signature = root / "checksums.txt.sig"
            certificate = root / "checksums.txt.pem"
            payload.write_bytes(b"payload")
            signature.write_bytes(b"invalid synthetic signature fixture")
            certificate.write_bytes(
                b"-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----\n"
            )
            files = {
                payload.name: payload,
                signature.name: signature,
                certificate.name: certificate,
            }
            rejected = verify_release.subprocess.CompletedProcess(
                args=["cosign"], returncode=1, stdout="", stderr="invalid signature"
            )
            with patch.object(verify_release.subprocess, "run", return_value=rejected):
                with self.assertRaisesRegex(SystemExit, "cosign verification failed"):
                    verify_release.verify_signatures(
                        {payload.name}, files,
                        "https://github.com/example/project/.github/workflows/release.yml@refs/tags/v1.0.0",
                    )


if __name__ == "__main__":
    unittest.main()
