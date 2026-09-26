#!/usr/bin/env python3
"""Offline negative controls for the pinned release asset verifier."""
from __future__ import annotations

import hashlib
import io
import json
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
VERIFIER = ROOT / "scripts/dist-oracle/verify-release.py"
REPOSITORY = "github.com/danieljustus/symaira-brain"
VERSION = "0.12.0"
RELEASE_BASE = f"https://{REPOSITORY}/releases/download/v{VERSION}/"


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def write_archive(path: Path, members: dict[str, bytes]) -> None:
    if path.name.endswith(".zip"):
        with zipfile.ZipFile(path, "w") as archive:
            for name, data in members.items():
                info = zipfile.ZipInfo(name)
                info.external_attr = (stat.S_IFREG | 0o644) << 16
                archive.writestr(info, data)
        return
    with tarfile.open(path, "w:gz") as archive:
        for name, data in members.items():
            info = tarfile.TarInfo(name)
            info.mode = 0o755 if name == "symbrain" else 0o644
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))


def make_bundle(root: Path) -> dict[str, Path]:
    assets = root / "assets"
    assets.mkdir()
    archives: list[str] = []
    sboms: list[str] = []
    for os_name, arch in (("darwin", "amd64"), ("darwin", "arm64"), ("linux", "amd64"), ("linux", "arm64"), ("windows", "amd64"), ("windows", "arm64")):
        suffix = "zip" if os_name == "windows" else "tar.gz"
        archive_name = f"symbrain_{VERSION}_{os_name}_{arch}.{suffix}"
        binary = "symbrain.exe" if os_name == "windows" else "symbrain"
        write_archive(assets / archive_name, {
            "AGENTS.md": b"synthetic docs\n",
            "LICENSE": b"synthetic license\n",
            "README.md": b"synthetic readme\n",
            binary: b"synthetic binary\n",
        })
        sbom_name = archive_name + ".sbom.json"
        (assets / sbom_name).write_text(json.dumps({
            "bomFormat": "CycloneDX",
            "specVersion": "1.6",
            "components": [{"name": archive_name}],
        }) + "\n", encoding="utf-8")
        archives.append(archive_name)
        sboms.append(sbom_name)

    checksum_names = archives + sboms
    checksum_data = "".join(
        hashlib.sha256((assets / name).read_bytes()).hexdigest() + "  " + name + "\n"
        for name in sorted(checksum_names)
    ).encode()
    (assets / "checksums.txt").write_bytes(checksum_data)

    asset_names = ["checksums.txt", "Symaira-Brain-0.12.0-macos.dmg"]
    asset_names.extend(archives + sboms)
    (assets / "Symaira-Brain-0.12.0-macos.dmg").write_bytes(b"synthetic dmg")

    manifest = {
        "version": VERSION,
        "tag": "v" + VERSION,
        "repository": REPOSITORY,
        "asset_count": len(asset_names),
        "assets": [
            {"name": name, "size": (assets / name).stat().st_size, "digest": digest((assets / name).read_bytes())}
            for name in asset_names
        ],
    }
    manifest_path = root / "manifest.json"
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

    formula_lines = []
    for name in sorted(item for item in archives if not "_windows_" in item):
        formula_lines.extend((f'url "{RELEASE_BASE}{name}"', f'sha256 "{hashlib.sha256((assets / name).read_bytes()).hexdigest()}"'))
    formula = root / "symbrain.rb"
    formula.write_text("\n".join(formula_lines) + "\n", encoding="utf-8")
    cask = root / "symbrain-cask.rb"
    cask.write_text(
        f'cask "symbrain" do\n  version "{VERSION}"\n  sha256 "{hashlib.sha256((assets / "Symaira-Brain-0.12.0-macos.dmg").read_bytes()).hexdigest()}"\n'
        f'  url "https://{REPOSITORY}/releases/download/v#{{version}}/Symaira-Brain-#{{version}}-macos.dmg"\nend\n',
        encoding="utf-8",
    )
    tap = {
        "version": VERSION,
        "commit": "a" * 40,
        "repository": "github.com/danieljustus/homebrew-tap",
        "files": {
            "Formula/symbrain.rb": digest(formula.read_bytes()),
            "Casks/symbrain.rb": digest(cask.read_bytes()),
        },
    }
    tap_path = root / "tap.json"
    tap_path.write_text(json.dumps(tap), encoding="utf-8")
    return {"assets": assets, "manifest": manifest_path, "formula": formula, "cask": cask, "tap": tap_path}


def refresh_manifest_digest(bundle: dict[str, Path], name: str) -> None:
    manifest = json.loads(bundle["manifest"].read_text(encoding="utf-8"))
    path = bundle["assets"] / name
    for item in manifest["assets"]:
        if item["name"] == name:
            data = path.read_bytes()
            item["size"] = len(data)
            item["digest"] = digest(data)
            break
    bundle["manifest"].write_text(json.dumps(manifest), encoding="utf-8")


def refresh_checksum(bundle: dict[str, Path], name: str) -> None:
    checksums = bundle["assets"] / "checksums.txt"
    rows = checksums.read_text(encoding="utf-8").splitlines()
    updated = []
    for row in rows:
        old_name = row.split("  ", 1)[1]
        updated.append((hashlib.sha256((bundle["assets"] / old_name).read_bytes()).hexdigest() if old_name == name else row.split("  ", 1)[0]) + "  " + old_name)
    checksums.write_text("\n".join(updated) + "\n", encoding="utf-8")
    refresh_manifest_digest(bundle, "checksums.txt")


def run_verifier(bundle: dict[str, Path], extra_args: tuple[str, ...] = ()) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(VERIFIER), "--assets", str(bundle["assets"]),
         "--formula", str(bundle["formula"]), "--cask", str(bundle["cask"]),
         "--manifest", str(bundle["manifest"]), "--tap-manifest", str(bundle["tap"]), *extra_args],
        capture_output=True,
        text=True,
        check=False,
    )


class ReleaseBundleVerifierTests(unittest.TestCase):
    def test_valid_synthetic_release_bundle_passes_offline(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            result = run_verifier(make_bundle(Path(raw)))
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertIn("checksums.txt entries", result.stdout)

    def test_tampered_checksum_is_rejected_after_asset_manifest_refresh(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = make_bundle(Path(raw))
            checksums = bundle["assets"] / "checksums.txt"
            rows = checksums.read_text(encoding="utf-8").splitlines()
            name = rows[0].split("  ", 1)[1]
            rows[0] = "0" * 64 + "  " + name
            checksums.write_text("\n".join(rows) + "\n", encoding="utf-8")
            refresh_manifest_digest(bundle, "checksums.txt")
            result = run_verifier(bundle)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("checksums.txt mismatch", result.stderr)

    def test_tampered_sbom_is_rejected_even_when_its_hashes_are_recomputed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = make_bundle(Path(raw))
            sbom = next(bundle["assets"].glob("*.sbom.json"))
            document = json.loads(sbom.read_text(encoding="utf-8"))
            document["bomFormat"] = "not-CycloneDX"
            sbom.write_text(json.dumps(document) + "\n", encoding="utf-8")
            refresh_manifest_digest(bundle, sbom.name)
            refresh_checksum(bundle, sbom.name)
            result = run_verifier(bundle)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("is not CycloneDX", result.stderr)

    def test_extra_archive_member_is_rejected_even_when_hashes_are_recomputed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = make_bundle(Path(raw))
            archive = next(bundle["assets"].glob("*_darwin_arm64.tar.gz"))
            members = {"AGENTS.md": b"synthetic docs\n", "LICENSE": b"synthetic license\n", "README.md": b"synthetic readme\n", "symbrain": b"synthetic binary\n", "extra": b"unexpected\n"}
            write_archive(archive, members)
            refresh_manifest_digest(bundle, archive.name)
            refresh_checksum(bundle, archive.name)
            result = run_verifier(bundle)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unexpected archive contents", result.stderr)

    def test_release_claim_requires_signature_and_sbom_verification(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = make_bundle(Path(raw))
            signature_missing = run_verifier(bundle, ("--release-claim",))
            self.assertNotEqual(signature_missing.returncode, 0)
            self.assertIn("release claim requires --verify-signatures", signature_missing.stderr)

            sbom_missing = run_verifier(bundle, ("--release-claim", "--verify-signatures"))
            self.assertNotEqual(sbom_missing.returncode, 0)
            self.assertIn("release claim requires --verify-sboms", sbom_missing.stderr)

    def test_release_claim_blocks_missing_signature_and_certificate_assets(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bundle = make_bundle(Path(raw))
            result = run_verifier(
                bundle,
                ("--release-claim", "--verify-signatures", "--verify-sboms"),
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing signature sidecar", result.stderr)


if __name__ == "__main__":
    unittest.main()
