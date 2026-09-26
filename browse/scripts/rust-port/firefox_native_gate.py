#!/usr/bin/env python3
"""Fetch and verify an official, isolated Firefox Nightly for native CI."""

from __future__ import annotations

import argparse
import hashlib
import html.parser
import json
import os
from pathlib import Path, PurePosixPath
import re
import struct
import subprocess
import tarfile
from urllib.parse import urlparse
import zipfile


BASE_URL = "https://archive.mozilla.org/pub/firefox/nightly/latest-mozilla-central"
TARGETS = {
    "darwin-amd64": ("macOS", "X64", "mac", "pkg", "x86_64", "mac"),
    "darwin-arm64": ("macOS", "ARM64", "mac", "pkg", "arm64", "mac"),
    "linux-amd64": ("Linux", "X64", "linux-x86_64", "tar.xz", "x86_64", "linux"),
    "linux-arm64": ("Linux", "ARM64", "linux-aarch64", "tar.xz", "aarch64", "linux"),
    "windows-amd64": ("Windows", "X64", "win64", "zip", "x86_64", "win"),
    "windows-arm64": ("Windows", "ARM64", "win64-aarch64", "zip", "aarch64", "win"),
}


class Links(html.parser.HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.hrefs: set[str] = set()

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag == "a":
            self.hrefs.update(
                PurePosixPath(value).name
                for key, value in attrs
                if key == "href" and value
            )


def version_key(version: str) -> tuple[int, ...]:
    match = re.fullmatch(r"(\d+(?:\.\d+)*)a(\d+)", version)
    if not match:
        raise ValueError(f"unsupported Nightly version: {version}")
    return tuple(int(part) for part in match.group(1).split(".")) + (int(match.group(2)),)


def artifact_candidates(names: set[str], platform: str, extension: str) -> list[tuple[tuple[int, ...], str, str]]:
    pattern = re.compile(
        rf"^firefox-(\d+(?:\.\d+)*)a(\d+)\.en-US\.{re.escape(platform)}\.{re.escape(extension)}$"
    )
    candidates: list[tuple[tuple[int, ...], str, str]] = []
    for name in names:
        match = pattern.fullmatch(name)
        if not match:
            continue
        version = f"{match.group(1)}a{match.group(2)}"
        stem = name[: -(len(extension) + 1)]
        if f"{stem}.checksums" in names and f"{stem}.buildhub.json" in names:
            candidates.append((version_key(version), version, name))
    return candidates


def latest_artifact(names: set[str], platform: str, extension: str) -> tuple[str, str]:
    candidates = artifact_candidates(names, platform, extension)
    if not candidates:
        raise ValueError(f"no checksummed Nightly {platform}.{extension} artifact in official index")
    _, version, name = max(candidates)
    return version, name


def exact_artifact(names: set[str], platform: str, extension: str, version: str) -> str:
    matches = [
        name
        for _, candidate_version, name in artifact_candidates(names, platform, extension)
        if candidate_version == version
    ]
    if len(matches) != 1:
        raise ValueError(f"expected one {version} Nightly {platform}.{extension} artifact; found {len(matches)}")
    return matches[0]


def checksum_for(contents: str, name: str) -> str:
    for line in contents.splitlines():
        fields = line.split()
        if len(fields) == 4 and fields[1] == "sha256" and fields[3] == name:
            digest = fields[0].lower()
            if re.fullmatch(r"[0-9a-f]{64}", digest):
                return digest
    raise ValueError(f"official SHA-256 checksum missing for {name}")


def runner_matches(target: str, runner_os: str, runner_arch: str) -> None:
    expected = TARGETS[target][:2]
    if (runner_os, runner_arch) != expected:
        raise ValueError(
            f"runner {runner_os}/{runner_arch} does not match required {target} ({expected[0]}/{expected[1]})"
        )


def safe_relative(name: str) -> PurePosixPath:
    path = PurePosixPath(name.replace("\\", "/"))
    if path.is_absolute() or re.match(r"^[A-Za-z]:", name) or ".." in path.parts:
        raise ValueError(f"archive contains an unsafe path: {name}")
    return path


def elf_machine(path: Path) -> int:
    header = path.read_bytes()[:20]
    if len(header) < 20 or header[:4] != b"\x7fELF":
        raise ValueError(f"not an ELF browser binary: {path}")
    endian = {1: "<", 2: ">"}.get(header[5])
    if endian is None:
        raise ValueError(f"invalid ELF byte order: {path}")
    return struct.unpack_from(f"{endian}H", header, 18)[0]


def pe_machine(path: Path) -> int:
    with path.open("rb") as stream:
        header = stream.read(4096)
    if len(header) < 64 or header[:2] != b"MZ":
        raise ValueError(f"not a PE browser binary: {path}")
    offset = struct.unpack_from("<I", header, 0x3C)[0]
    if offset + 6 > len(header) or header[offset : offset + 4] != b"PE\0\0":
        raise ValueError(f"invalid PE header: {path}")
    return struct.unpack_from("<H", header, offset + 4)[0]


def verify_executable_version(stdout: str, version: str) -> None:
    """Check the executable's version line while allowing runtime warnings on stderr."""
    first_line = stdout.splitlines()[0].strip() if stdout.splitlines() else ""
    expected = f"Mozilla Firefox {version}"
    if first_line != expected:
        raise ValueError(f"executable Nightly identity mismatch: expected {expected!r}; got {first_line!r}")


def download(name: str, destination: Path) -> None:
    if name != "index.html" and Path(name).name != name:
        raise ValueError(f"unsafe artifact name: {name}")
    url = f"{BASE_URL}/" if name == "index.html" else f"{BASE_URL}/{name}"
    subprocess.run(
        ["curl", "--fail", "--location", "--silent", "--show-error", "--retry", "3", url, "--output", str(destination)],
        check=True,
    )


def verify_metadata(metadata: dict, platform: str, version: str, target_os: str) -> str:
    target = metadata.get("target", {})
    source = metadata.get("source", {})
    download_url = metadata.get("download", {}).get("url", "")
    parsed_url = urlparse(download_url)
    if (
        source.get("product") != "firefox"
        or source.get("repository") != "https://hg.mozilla.org/mozilla-central"
        or source.get("tree") != "mozilla-central"
        or target.get("channel") != "nightly"
        or target.get("locale") != "en-US"
        or target.get("os") != target_os
        or target.get("platform") != platform
        or target.get("version") != version
        or parsed_url.scheme != "https"
        or parsed_url.hostname != "archive.mozilla.org"
        or not parsed_url.path.startswith("/pub/firefox/nightly/")
        or not re.fullmatch(r"[0-9a-f]{40}", source.get("revision", ""))
    ):
        raise ValueError(f"Mozilla Buildhub identity mismatch: {metadata}")
    return source["revision"]


def extract_artifact(archive: Path, destination: Path, extension: str, target: str) -> Path:
    if extension == "pkg":
        expanded = destination / "expanded"
        subprocess.run(["pkgutil", "--expand-full", str(archive), str(expanded)], check=True)
        executable = expanded / "Firefox Nightly.pkg/Payload/Firefox Nightly.app/Contents/MacOS/firefox"
        if not executable.is_file():
            raise ValueError("Mozilla macOS package did not contain Firefox Nightly.app")
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(executable.parents[2])], check=True)
        return executable

    unpacked = destination / "unpacked"
    unpacked.mkdir()
    if extension == "tar.xz":
        with tarfile.open(archive, "r:xz") as package:
            members = package.getmembers()
            for member in members:
                safe_relative(member.name)
                if member.issym() or member.islnk():
                    safe_relative(member.linkname)
            package.extractall(unpacked, members=members)
        executable = unpacked / "firefox/firefox"
        binary = unpacked / "firefox/firefox-bin"
        expected_machine = {"linux-amd64": 62, "linux-arm64": 183}[target]
        if elf_machine(binary) != expected_machine:
            raise ValueError(f"Linux Firefox ELF machine does not match {target}")
    else:
        with zipfile.ZipFile(archive) as package:
            for member in package.infolist():
                safe_relative(member.filename)
            package.extractall(unpacked)
        matches = list(unpacked.rglob("firefox.exe"))
        if len(matches) != 1:
            raise ValueError(f"expected one firefox.exe in Mozilla ZIP; found {len(matches)}")
        executable = matches[0]
        expected_machine = {"windows-amd64": 0x8664, "windows-arm64": 0xAA64}[target]
        if pe_machine(executable) != expected_machine:
            raise ValueError(f"Windows Firefox PE machine does not match {target}")
    if not executable.is_file():
        raise ValueError(f"Mozilla archive did not contain a Firefox executable: {executable}")
    return executable


def prepare(target: str, destination: Path) -> None:
    if target not in TARGETS:
        raise ValueError(f"unsupported native target: {target}")
    runner_matches(target, os.environ.get("RUNNER_OS", ""), os.environ.get("RUNNER_ARCH", ""))
    if destination.exists():
        raise ValueError(f"refusing to reuse Nightly directory: {destination}")
    destination.mkdir(parents=True)

    index_path = destination / "index.html"
    download("index.html", index_path)
    if index_path.stat().st_size == 0:
        raise ValueError("Mozilla Nightly index was empty")
    links = Links()
    links.feed(index_path.read_text(encoding="utf-8"))
    _, _, platform, extension, native_arch, target_os = TARGETS[target]
    version = os.environ.get("NIGHTLY_VERSION", "")
    revision = os.environ.get("NIGHTLY_SOURCE_REVISION", "")
    if not version or not revision:
        raise ValueError("the shared Firefox Nightly version and source revision are required")
    artifact_name = exact_artifact(links.hrefs, platform, extension, version)
    stem = artifact_name[: -(len(extension) + 1)]

    checksums_path = destination / "checksums"
    metadata_path = destination / "buildhub.json"
    download(f"{stem}.checksums", checksums_path)
    download(f"{stem}.buildhub.json", metadata_path)
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    artifact_revision = verify_metadata(metadata, platform, version, target_os)
    if artifact_revision != revision:
        raise ValueError(
            f"Nightly source revision changed after planning: expected {revision}, got {artifact_revision}"
        )

    archive = destination / artifact_name
    download(artifact_name, archive)
    expected_digest = checksum_for(checksums_path.read_text(encoding="utf-8"), artifact_name)
    digest = hashlib.sha256()
    with archive.open("rb") as package:
        for block in iter(lambda: package.read(1024 * 1024), b""):
            digest.update(block)
    actual_digest = digest.hexdigest()
    if actual_digest != expected_digest:
        raise ValueError(f"official Nightly SHA-256 mismatch for {artifact_name}")

    executable = extract_artifact(archive, destination, extension, target)
    if platform == "mac":
        arches = subprocess.check_output(["lipo", "-archs", str(executable)], text=True).split()
        if native_arch not in arches:
            raise ValueError(f"universal macOS Firefox lacks native {native_arch} slice")
        command = ["arch", f"-{native_arch}", str(executable), "--version"]
    else:
        command = [str(executable), "--version"]
    result = subprocess.run(command, check=True, capture_output=True, text=True, timeout=30)
    verify_executable_version(result.stdout, version)
    print(
        f"Verified Firefox Nightly {version} ({target}), Mozilla source {revision}, "
        f"SHA-256 {actual_digest}, native binary {executable}"
    )

    github_env = os.environ.get("GITHUB_ENV")
    if not github_env:
        raise ValueError("GITHUB_ENV is required to pass the verified executable to the native test")
    with Path(github_env).open("a", encoding="utf-8") as output_env:
        output_env.write(f"SYMBROWSE_FIREFOX_EXECUTABLE={executable}\n")


def plan_shared_nightly(destination: Path) -> None:
    if destination.exists():
        raise ValueError(f"refusing to reuse Nightly plan directory: {destination}")
    destination.mkdir(parents=True)
    index_path = destination / "index.html"
    download("index.html", index_path)
    links = Links()
    links.feed(index_path.read_text(encoding="utf-8"))

    artifact_specs = sorted({(values[2], values[3], values[5]) for values in TARGETS.values()})
    available: list[set[tuple[str, str]]] = []
    for platform, extension, target_os in artifact_specs:
        candidates = artifact_candidates(links.hrefs, platform, extension)
        revisions: set[tuple[str, str]] = set()
        for _, version, artifact in candidates:
            stem = artifact[: -(len(extension) + 1)]
            metadata_name = f"{stem}.buildhub.json"
            metadata_path = destination / metadata_name
            if not metadata_path.exists():
                download(metadata_name, metadata_path)
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            revision = verify_metadata(metadata, platform, version, target_os)
            revisions.add((version, revision))
        if not revisions:
            raise ValueError(f"official Nightly index has no verified build for {platform}.{extension}")
        available.append(revisions)

    common = set.intersection(*available)
    if not common:
        raise ValueError("no common Firefox Nightly version and Mozilla source revision across all native artifacts")
    version, revision = max(common, key=lambda item: (version_key(item[0]), item[1]))
    print(f"Planning all six Firefox native jobs at Nightly {version}, Mozilla source {revision}")
    github_output = os.environ.get("GITHUB_OUTPUT")
    if not github_output:
        raise ValueError("GITHUB_OUTPUT is required to share the Nightly identity with matrix jobs")
    with Path(github_output).open("a", encoding="utf-8") as output:
        output.write(f"version={version}\nsource_revision={revision}\n")


def main() -> None:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--prepare", action="store_true")
    mode.add_argument("--plan", action="store_true")
    parser.add_argument("--target", choices=sorted(TARGETS))
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    if args.prepare:
        if not args.target:
            parser.error("--prepare requires --target")
        prepare(args.target, args.destination)
    else:
        if args.target:
            parser.error("--plan cannot be combined with --target")
        plan_shared_nightly(args.destination)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        raise SystemExit(f"Firefox native prerequisite failed: {error}") from error
