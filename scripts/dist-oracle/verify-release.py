#!/usr/bin/env python3
"""Verify downloaded release assets against the pinned capture and tap files.

Example:
  python3 scripts/dist-oracle/verify-release.py --assets /path/to/release \
    --formula /path/to/symbrain.rb --cask /path/to/symbrain-cask.rb \
    --verify-signatures --verify-sboms

The checker is opt-in and read-only. It does not download, install, rebuild,
publish, or clean artifacts. Keep the verified assets as evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import stat
import subprocess
import tarfile
import zipfile
from pathlib import Path


HERE = Path(__file__).resolve().parent
DEFAULT_MANIFEST = HERE / "fixtures" / "manifest_v0.12.0.json"
DEFAULT_TAP_MANIFEST = HERE / "fixtures" / "tap_v0.12.0.json"
OIDC_ISSUER = "https://token.actions.githubusercontent.com"


def require(ok: bool, message: str) -> None:
    if not ok:
        raise SystemExit(f"FAIL: {message}")


def require_pinned_bytes(data: bytes, expected: str, label: str) -> None:
    actual = "sha256:" + hashlib.sha256(data).hexdigest()
    require(actual == expected, f"{label} differs from pinned live tap bytes: {actual}")


def verify_tap_links(formula: str, cask: str, manifest: dict) -> tuple[int, int]:
    version = manifest["version"]
    repo_url = f"https://{manifest['repository']}/releases/download/v{version}/"
    assets = {asset["name"]: asset for asset in manifest["assets"]}
    expected_formula = {
        name: asset["digest"].removeprefix("sha256:")
        for name, asset in assets.items()
        if name.endswith((".tar.gz", ".zip")) and "_windows_" not in name
    }
    expected_formula_urls = {repo_url + name for name in expected_formula}
    all_formula_urls = re.findall(r'^\s*url "([^"]*/releases/download/[^"]+)"\s*$', formula, re.MULTILINE)
    require(len(all_formula_urls) == len(expected_formula_urls) and set(all_formula_urls) == expected_formula_urls, "Homebrew formula has extra or missing release-download URLs")
    formula_pairs = re.findall(
        r'^\s*url "(' + re.escape(repo_url) + r'([^"]+))"\s*\n\s*sha256 "([0-9a-f]{64})"',
        formula,
        re.MULTILINE,
    )
    actual_formula = {name: digest for _, name, digest in formula_pairs}
    require(len(formula_pairs) == len(expected_formula), "Homebrew formula has missing or duplicate release links")
    require(actual_formula == expected_formula, "Homebrew formula URLs/checksums differ from the release manifest")

    dmg_name = f"Symaira-Brain-{version}-macos.dmg"
    dmg_digest = assets[dmg_name]["digest"].removeprefix("sha256:")
    expected_cask_url = f"https://{manifest['repository']}/releases/download/v#{{version}}/Symaira-Brain-#{{version}}-macos.dmg"
    all_cask_release_urls = re.findall(r'^\s*url "([^"]*/releases/download/[^"]+)"\s*$', cask, re.MULTILINE)
    require(all_cask_release_urls == [expected_cask_url], "Homebrew cask has extra or missing release-download URLs")
    require(re.findall(r'^\s*version "([^"]+)"\s*$', cask, re.MULTILINE) == [version], "Homebrew cask version differs from the release manifest")
    require(re.findall(r'^\s*sha256 "([0-9a-f]{64})"\s*$', cask, re.MULTILINE) == [dmg_digest], "Homebrew cask checksum differs from the release manifest")
    cask_urls = re.findall(r'^\s*url "([^"]+)"\s*$', cask, re.MULTILINE)
    require(cask_urls.count(expected_cask_url) == 1, "Homebrew cask URL differs from the release manifest")
    return len(expected_formula), 1


def exact_asset_files(asset_dir: Path, expected_names: set[str]) -> dict[str, Path]:
    entries = list(asset_dir.iterdir())
    ensure_exact_asset_names([path.name for path in entries], expected_names)
    for path in entries:
        require(path.is_file() and not path.is_symlink(), f"release asset is not a regular file: {path.name}")
    return {path.name: path for path in entries}


def ensure_exact_asset_names(names: list[str], expected_names: set[str]) -> None:
    names = set(names)
    require(names == expected_names, f"asset directory mismatch: missing={sorted(expected_names - names)} extra={sorted(names - expected_names)}")


def archive_file_names(asset_dir: Path, name: str) -> list[str]:
    path = asset_dir / name
    if name.endswith(".tar.gz"):
        with tarfile.open(path, "r:gz") as archive:
            members = archive.getmembers()
            require(all(item.isfile() for item in members), f"unexpected non-file entry in {name}")
            return [item.name for item in members]
    with zipfile.ZipFile(path) as archive:
        return zip_file_names(archive, name)


def zip_file_names(archive: zipfile.ZipFile, name: str) -> list[str]:
    members = archive.infolist()
    for item in members:
        mode = item.external_attr >> 16
        file_type = stat.S_IFMT(mode)
        require(not item.is_dir() and file_type in (0, stat.S_IFREG), f"non-regular ZIP entry in {name}: {item.filename}")
    return [item.filename for item in members]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", required=True, type=Path, help="directory containing only the downloaded release assets")
    parser.add_argument("--formula", required=True, type=Path, help="read-only Homebrew formula file")
    parser.add_argument("--cask", required=True, type=Path, help="read-only Homebrew cask file")
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--tap-manifest", type=Path, default=DEFAULT_TAP_MANIFEST)
    parser.add_argument("--verify-signatures", action="store_true", help="verify cosign signatures, certificate identity, issuer, and Rekor inclusion")
    parser.add_argument("--verify-sboms", action="store_true", help="parse each SBOM with the syft CLI")
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text())
    assets = {item["name"]: item for item in manifest["assets"]}
    require(len(assets) == len(manifest["assets"]), "manifest contains duplicate asset names")
    require(len(assets) == manifest["asset_count"], "manifest asset_count is inconsistent")
    require(manifest["tag"] == f"v{manifest['version']}", "manifest tag/version mismatch")
    files = exact_asset_files(args.assets, set(assets))
    for name, asset in assets.items():
        path = files.get(name)
        require(path is not None, f"missing release asset {name}")
        require(path.stat().st_size == asset["size"], f"size mismatch for {name}")
        digest = "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()
        require(digest == asset["digest"], f"SHA-256 mismatch for {name}")
    print(f"ok: {len(assets)} release asset names, sizes, and SHA-256 digests")

    expected_checksums = {
        name for name in assets
        if name.endswith((".tar.gz", ".zip", ".sbom.json"))
    }
    checksum_lines = (args.assets / "checksums.txt").read_text().splitlines()
    checksums: dict[str, str] = {}
    for line in checksum_lines:
        match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
        require(match is not None, "checksums.txt has an invalid line")
        digest, name = match.groups()
        require(name not in checksums, f"duplicate checksum entry for {name}")
        checksums[name] = digest
    require(set(checksums) == expected_checksums, "checksums.txt payload set differs from archives and SBOMs")
    for name, digest in checksums.items():
        actual = hashlib.sha256((args.assets / name).read_bytes()).hexdigest()
        require(actual == digest, f"checksums.txt mismatch for {name}")
    print(f"ok: {len(checksums)} checksums.txt entries")

    signed = {"checksums.txt"} | {
        name for name in assets if name.endswith((".tar.gz", ".zip", ".sbom.json"))
    }
    if args.verify_signatures:
        identity = f"https://{manifest['repository']}/.github/workflows/release.yml@refs/tags/{manifest['tag']}"
        for name in sorted(signed):
            payload = args.assets / name
            cmd = [
                "cosign", "verify-blob", "--certificate", str(payload) + ".pem",
                "--signature", str(payload) + ".sig", "--certificate-identity", identity,
                "--certificate-oidc-issuer", OIDC_ISSUER, str(payload),
            ]
            result = subprocess.run(cmd, capture_output=True, text=True, check=False)
            require(result.returncode == 0, f"cosign verification failed for {name}: {result.stderr.strip()}")
        print(f"ok: {len(signed)} cosign signatures, workflow identity, OIDC issuer, and transparency proofs")

    archive_names = {name for name in assets if name.endswith((".tar.gz", ".zip"))}
    for name in sorted(archive_names):
        names = archive_file_names(args.assets, name)
        binary = "symbrain.exe" if "_windows_" in name else "symbrain"
        require(len(names) == len(set(names)), f"duplicate archive entries in {name}")
        require(set(names) == {"AGENTS.md", "LICENSE", "README.md", binary}, f"unexpected archive contents in {name}: {names}")
    print(f"ok: {len(archive_names)} archive file sets and binary names")

    sboms = sorted(name for name in assets if name.endswith(".sbom.json"))
    for name in sboms:
        doc = json.loads((args.assets / name).read_text())
        require(doc.get("bomFormat") == "CycloneDX", f"{name} is not CycloneDX")
        require(isinstance(doc.get("specVersion"), str) and doc["specVersion"], f"{name} has no CycloneDX specVersion")
        components = doc.get("components")
        require(isinstance(components, list) and components, f"{name} has no package components")
        if args.verify_sboms:
            result = subprocess.run(["syft", "convert", str(args.assets / name), "-o", "syft-json"], capture_output=True, check=False)
            require(result.returncode == 0, f"syft could not parse {name}: {result.stderr.decode(errors='replace').strip()}")
    suffix = " (syft parser passed)" if args.verify_sboms else ""
    print(f"ok: {len(sboms)} CycloneDX SBOMs with components{suffix}")

    version = manifest["version"]
    tap_manifest = json.loads(args.tap_manifest.read_text())
    require(tap_manifest["version"] == version, "tap snapshot version differs from release manifest")
    require(re.fullmatch(r"[0-9a-f]{40}", tap_manifest["commit"]) is not None, "tap snapshot is not pinned to a commit")
    require(tap_manifest["repository"] == "github.com/danieljustus/homebrew-tap", "tap snapshot repository mismatch")
    tap_files = tap_manifest["files"]
    require_pinned_bytes(args.formula.read_bytes(), tap_files["Formula/symbrain.rb"], "Homebrew formula")
    require_pinned_bytes(args.cask.read_bytes(), tap_files["Casks/symbrain.rb"], "Homebrew cask")
    formula_count, cask_count = verify_tap_links(args.formula.read_text(), args.cask.read_text(), manifest)
    print(f"ok: Homebrew tap snapshot {tap_manifest['commit']}; formula links {formula_count}, cask links {cask_count}")


if __name__ == "__main__":
    main()
