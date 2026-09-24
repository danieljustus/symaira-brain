#!/usr/bin/env python3
"""Fetch an official Chrome for Testing build into an isolated test directory."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import subprocess
import sys
import zipfile
from pathlib import Path


METADATA_URL = (
    "https://googlechromelabs.github.io/chrome-for-testing/"
    "last-known-good-versions-with-downloads.json"
)
SUPPORTED = {"linux64", "linux-arm64", "mac-x64", "mac-arm64", "win64"}


def github_output(values: dict[str, str]) -> None:
    path = os.environ.get("GITHUB_OUTPUT")
    if path:
        with open(path, "a", encoding="utf-8") as output:
            for key, value in values.items():
                output.write(f"{key}={value}\n")


def fetch_metadata() -> dict[str, object]:
    result = subprocess.run(
        ["curl", "--fail", "--location", "--silent", "--show-error", "--retry", "3", METADATA_URL],
        capture_output=True, timeout=60, check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"CfT metadata download failed with curl exit {result.returncode}")
    return json.loads(result.stdout)


def chrome_binary(platform: str, root: Path) -> Path:
    if platform in {"linux64", "linux-arm64"}:
        return root / f"chrome-{platform}" / "chrome"
    if platform in {"mac-x64", "mac-arm64"}:
        return (root / f"chrome-{platform}" / "Google Chrome for Testing.app"
                / "Contents" / "MacOS" / "Google Chrome for Testing")
    return root / "chrome-win64" / "chrome.exe"


def extract_safely(archive: Path, destination: Path) -> None:
    base = destination.resolve()
    with zipfile.ZipFile(archive) as bundle:
        members = bundle.infolist()
        for member in members:
            target = (destination / member.filename).resolve()
            if not target.is_relative_to(base):
                raise RuntimeError(f"CfT archive entry escapes destination: {member.filename!r}")
        bundle.extractall(destination)
        for member in members:
            mode = member.external_attr >> 16
            if mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH):
                (destination / member.filename).chmod(mode & 0o777)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", required=True,
                        choices=sorted(SUPPORTED | {"win-arm64"}))
    parser.add_argument("--destination", required=True, type=Path)
    args = parser.parse_args()
    metadata = fetch_metadata()
    stable = metadata["channels"]["Stable"]
    if args.platform == "win-arm64":
        downloads = stable["downloads"]["chrome"]
        candidates = [item for item in downloads if item["platform"] == args.platform]
        if candidates:
            raise RuntimeError(
                "official CfT metadata now publishes Windows ARM64 Chrome; update this native E2E "
                "to execute the published artifact instead of asserting unavailable behavior"
            )
        missing_binary = str(args.destination / "chrome-win-arm64" / "chrome.exe")
        reason = (
            f"official Chrome for Testing Stable {stable['version']} metadata does not publish "
            "a Windows ARM64 Chrome artifact; "
            "the native test will verify explicit unavailable behavior without browser substitution"
        )
        github_output({"available": "false", "reason": reason, "executable": missing_binary,
                       "metadata": METADATA_URL, "version": stable["version"]})
        print(json.dumps({"available": False, "platform": args.platform,
                          "version": stable["version"], "metadata": METADATA_URL,
                          "reason": reason}, sort_keys=True))
        return 0

    downloads = stable["downloads"]["chrome"]
    candidates = [item for item in downloads if item["platform"] == args.platform]
    if len(candidates) != 1:
        raise RuntimeError(f"official stable metadata has {len(candidates)} Chrome assets for {args.platform}")
    url = candidates[0]["url"]
    if not url.startswith("https://storage.googleapis.com/chrome-for-testing-public/"):
        raise RuntimeError(f"unexpected CfT download host: {url}")

    args.destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    archive = args.destination / "chrome-for-testing.zip"
    download = subprocess.run(
        ["curl", "--fail", "--location", "--silent", "--show-error", "--retry", "3",
         "--output", str(archive), url],
        capture_output=True, text=True, timeout=300, check=False,
    )
    if download.returncode != 0:
        raise RuntimeError(f"CfT archive download failed with curl exit {download.returncode}: {download.stderr.strip()}")
    archive_sha256 = sha256(archive)
    extract_safely(archive, args.destination)
    binary = chrome_binary(args.platform, args.destination)
    if not binary.is_file():
        raise RuntimeError(f"official CfT archive omitted expected executable: {binary}")
    isolated_home = args.destination / "profile-home"
    isolated_home.mkdir(mode=0o700)
    probe_env = os.environ.copy()
    probe_env.update({"HOME": str(isolated_home), "USERPROFILE": str(isolated_home)})
    result = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                            timeout=30, check=False, env=probe_env)
    version = result.stdout.strip() or result.stderr.strip()
    if result.returncode != 0 or stable["version"] not in version:
        raise RuntimeError(
            f"CfT executable version check failed: exit={result.returncode}, "
            f"expected={stable['version']!r}, output={version!r}"
        )
    github_output({"available": "true", "executable": str(binary), "version": stable["version"],
                   "archive_sha256": archive_sha256, "metadata": METADATA_URL})
    print(json.dumps({"available": True, "platform": args.platform,
                      "version": stable["version"], "url": url, "archive_sha256": archive_sha256,
                      "executable": str(binary), "metadata": METADATA_URL}, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"chrome-for-testing setup failed: {error}", file=sys.stderr)
        raise SystemExit(1)
