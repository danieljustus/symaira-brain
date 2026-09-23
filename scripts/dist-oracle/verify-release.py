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
DEFAULT_MANIFEST = HERE / "fixtures" / "manifest_v0.11.0.json"
OIDC_ISSUER = "https://token.actions.githubusercontent.com"


def require(ok: bool, message: str) -> None:
    if not ok:
        raise SystemExit(f"FAIL: {message}")


def exact_asset_files(asset_dir: Path, expected_names: set[str]) -> dict[str, Path]:
    entries = list(asset_dir.iterdir())
    ensure_exact_asset_names([path.name for path in entries], expected_names)
    for path in entries:
        require(path.is_file() and not path.is_symlink(), f"release asset is not a regular file: {path.name}")
    return {path.name: path for path in entries}


def ensure_exact_asset_names(names: list[str], expected_names: set[str]) -> None:
    names = set(names)
    require(names == expected_names, f"asset directory mismatch: missing={sorted(expected_names - names)} extra={sorted(names - expected_names)}")


def ruby_assignments(source: str) -> list[tuple[tuple[str, ...], str, str]]:
    """Read active url/sha256/version calls with their Ruby block scopes."""
    stack: list[str] = []
    found: list[tuple[tuple[str, ...], str, str]] = []
    opens = re.compile(r"^(?:(?:class|module|def|if|unless|case|begin|while|until|for)\b.*|.*\bdo(?:\s*\|[^|]*\|)?\s*(?:#.*)?)$")
    assignment = re.compile(r'^(url|sha256|version)\s+"([^"]*)"(?:\s+#.*)?$')
    for raw in source.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        close = re.fullmatch(r"end(?:\s+#.*)?", line)
        if close:
            require(bool(stack), "unbalanced end in Homebrew Ruby source")
            stack.pop()
            continue
        match = assignment.fullmatch(line)
        if match:
            found.append((tuple(stack), match.group(1), match.group(2)))
        if opens.fullmatch(line):
            stack.append(line)
    require(not stack, "unclosed block in Homebrew Ruby source")
    return found


def ruby_url_checksum_pairs(
    source: str, allowed_scopes: set[tuple[str, ...]]
) -> list[tuple[tuple[str, ...], str, str]]:
    calls = ruby_assignments(source)
    pairs = []
    for index, (scope, name, value) in enumerate(calls[:-1]):
        if name != "url" or scope not in allowed_scopes:
            continue
        next_scope, next_name, digest = calls[index + 1]
        require(next_scope == scope and next_name == "sha256", f"active Homebrew URL has no adjacent checksum: {value}")
        pairs.append((scope, value, digest))
    return pairs


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
    repo_url = f"https://{manifest['repository']}/releases/download/v{version}/"
    formula = args.formula.read_text()
    formula_scopes = {
        ("class Symbrain < Formula", "on_macos do", "if Hardware::CPU.intel?"),
        ("class Symbrain < Formula", "on_macos do", "if Hardware::CPU.arm?"),
        ("class Symbrain < Formula", "on_linux do", "if Hardware::CPU.intel? && Hardware::CPU.is_64_bit?"),
        ("class Symbrain < Formula", "on_linux do", "if Hardware::CPU.arm? && Hardware::CPU.is_64_bit?"),
    }
    expected_formula = {
        name: assets[name]["digest"].removeprefix("sha256:")
        for name in archive_names if "_windows_" not in name
    }
    formula_links = ruby_url_checksum_pairs(formula, formula_scopes)
    formula_by_name = {url.removeprefix(repo_url): digest for _, url, digest in formula_links if url.startswith(repo_url)}
    require(len(formula_links) == len(expected_formula) and len(formula_by_name) == len(formula_links), "Homebrew formula has duplicate or missing active release links")
    require(formula_by_name == expected_formula, "Homebrew formula release URLs/checksums differ from captured release")
    cask = args.cask.read_text()
    dmg_name = f"Symaira-Brain-{version}-macos.dmg"
    dmg_digest = assets[dmg_name]["digest"].removeprefix("sha256:")
    cask_scope = ("cask \"symbrain\" do",)
    cask_values = [(name, value) for scope, name, value in ruby_assignments(cask) if scope == cask_scope]
    require(cask_values.count(("version", version)) == 1, "Homebrew cask version mismatch")
    require(cask_values.count(("sha256", dmg_digest)) == 1, "Homebrew cask DMG checksum mismatch")
    cask_url = f"https://{manifest['repository']}/releases/download/v#{{version}}/Symaira-Brain-#{{version}}-macos.dmg"
    require(cask_values.count(("url", cask_url)) == 1, "Homebrew cask release URL mismatch")
    print(f"ok: Homebrew formula {len(expected_formula)} release links and cask DMG link/checksum")


if __name__ == "__main__":
    main()
