#!/usr/bin/env python3
"""Report pinned managed-core versions that trail their repo's latest release.

Port of the former Go `cmd/manifest-drift-check`. It never bumps the manifest:
a bump needs independently downloaded and hashed release archives. A fetch
failure or a repo without releases is UNKNOWN, never drift. Exit 1 only when a
core is BEHIND.
"""
from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

MANIFEST = Path(__file__).resolve().parents[1] / "rust/symbrain-managed/assets/manifest.json"


def latest_tag(repo: str, token: str) -> str:
    request = urllib.request.Request(f"https://api.github.com/repos/{repo}/releases/latest",
                                     headers={"Accept": "application/vnd.github+json"})
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return json.load(response).get("tag_name", "")
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return ""  # no releases published
        raise


def normalize(version: str) -> str:
    return version.strip().removeprefix("v")


def main() -> int:
    cores = json.loads(MANIFEST.read_text(encoding="utf-8"))["cores"]
    token = os.environ.get("GITHUB_TOKEN", "")
    behind = unknown = 0
    for name in sorted(cores):
        pinned = cores[name]["version"]
        try:
            tag = latest_tag(cores[name]["repo"], token)
            reason = "no releases found"
        except (OSError, ValueError) as error:
            tag, reason = "", str(error)
        if not tag:
            print(f"UNKNOWN  {name:<12} pinned={pinned:<12} ({reason})")
            unknown += 1
        elif normalize(tag) != normalize(pinned):
            print(f"BEHIND   {name:<12} pinned={pinned:<12} latest={tag}")
            behind += 1
        else:
            print(f"CURRENT  {name:<12} pinned={pinned}")
    if unknown:
        print(f"manifest-drift-check: {unknown} core(s) could not be checked (network/rate-limit) — not treated as drift",
              file=sys.stderr)
    if behind:
        print(f"manifest-drift-check: {behind} core(s) behind their latest release", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
