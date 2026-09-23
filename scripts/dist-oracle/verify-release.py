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
import subprocess
import tarfile
import zipfile
from pathlib import Path


HERE = Path(__file__).resolve().parent
DEFAULT_MANIFEST = HERE / "fixtures" / "manifest_v0.11.0.json"
OIDC_ISSUER = "https://token.actions.githubusercontent.com"


def require(ok: bool, message: str) -> None:
    if not ok:
        raise SystemExit(f"FAIL: {message}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", required=True, type=Path, help="directory containing downloaded release assets")
    parser.add_argument("--formula", required=True, type=Path, help="read-only Homebrew formula file")
    parser.add_argument("--cask", required=True, type=Path, help="read-only Homebrew cask file")
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--verify-signatures", action="store_true", help="verify cosign signatures, certificate identity, issuer, and Rekor inclusion")
    parser.add_argument("--verify-sboms", action="store_true", help="parse each SBOM with the syft CLI")
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text())
    assets = {item["name"]: item for item in manifest["assets"]}
    files = {path.name: path for path in args.assets.iterdir() if path.is_file()}
    require(len(assets) == manifest["asset_count"], "manifest asset_count is inconsistent")
    require(manifest["tag"] == f"v{manifest['version']}", "manifest tag/version mismatch")
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
        if name.endswith(".tar.gz"):
            with tarfile.open(args.assets / name, "r:gz") as archive:
                members = archive.getmembers()
                names = [item.name for item in members]
                require(all(item.isfile() for item in members), f"unexpected non-file entry in {name}")
        else:
            with zipfile.ZipFile(args.assets / name) as archive:
                names = archive.namelist()
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
    repo_url = f"https://{manifest['repository']}/releases/download/v{version}/"
    formula = args.formula.read_text()
    formula_links = re.findall(
        r'^\s*url "(' + re.escape(repo_url) + r'([^"]+))"\s*\n\s*sha256 "([0-9a-f]{64})"',
        formula,
        re.MULTILINE,
    )
    expected_formula = {
        name: assets[name]["digest"].removeprefix("sha256:")
        for name in archive_names if "_windows_" not in name
    }
    require(len(formula_links) == len(expected_formula), "Homebrew formula has duplicate or missing release links")
    require({name: digest for _, name, digest in formula_links} == expected_formula, "Homebrew formula release URLs/checksums differ from captured release")
    cask = args.cask.read_text()
    dmg_name = f"Symaira-Brain-{version}-macos.dmg"
    dmg_digest = assets[dmg_name]["digest"].removeprefix("sha256:")
    require(re.search(rf'^\s*version "{re.escape(version)}"\s*$', cask, re.MULTILINE) is not None, "Homebrew cask version mismatch")
    require(re.search(rf'^\s*sha256 "{dmg_digest}"\s*$', cask, re.MULTILINE) is not None, "Homebrew cask DMG checksum mismatch")
    cask_url = f"https://{manifest['repository']}/releases/download/v#{{version}}/Symaira-Brain-#{{version}}-macos.dmg"
    require(cask_url in cask, "Homebrew cask release URL mismatch")
    print(f"ok: Homebrew formula {len(expected_formula)} release links and cask DMG link/checksum")


if __name__ == "__main__":
    main()
