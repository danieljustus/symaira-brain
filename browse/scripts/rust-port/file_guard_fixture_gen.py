#!/usr/bin/env python3
"""Regenerate the source-bound Go file-guard fixture used by the Rust port."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[3] / "scripts"
sys.path.insert(0, str(SCRIPTS))
from external_env import ensure_external_environment

ensure_external_environment(__file__)

ORACLE_COMMIT = "dc9c54e41beccf131fbe70e3f45bfff98871e709"
SOURCE_FILES = (
    "internal/engine/chrome/files.go",
    "internal/engine/chrome/runtime_events.go",
    "internal/engine/files.go",
)
FIXTURE = Path("testdata/port/engine/file-guards.json")
MANIFEST = Path("testdata/port/engine/file-guards-manifest.json")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def source_hashes(root: Path) -> dict[str, str]:
    result = {}
    for relative in SOURCE_FILES:
        current = (root / relative).read_bytes()
        pinned = subprocess.check_output(
            ["git", "show", f"{ORACLE_COMMIT}:browse/{relative}"], cwd=root
        )
        if current != pinned:
            raise SystemExit(f"oracle source differs from pinned commit: {relative}")
        result[relative] = digest(current)
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    sources = source_hashes(root)
    with tempfile.TemporaryDirectory(prefix="symbrowse-file-guards-") as temporary:
        generated = Path(temporary) / "file-guards.json"
        env = os.environ.copy()
        env.update(CGO_ENABLED="0", GOTOOLCHAIN="go1.26.6", RUST_PORT_FILE_FIXTURE_OUT=str(generated))
        subprocess.run(
            ["go", "test", "-count=1", "./internal/engine/chrome", "-run", "^TestGenerateRustPortFileFixture$"],
            cwd=root,
            env=env,
            check=True,
        )
        content = generated.read_bytes()
    data = json.loads(content)
    manifest = {
        "schema_version": 1,
        "oracle_commit": ORACLE_COMMIT,
        "source_files": sources,
        "generator_sha256": digest(Path(__file__).read_bytes()),
        "fixture": str(FIXTURE),
        "upload_case_count": len(data["upload"]),
        "download_collision_handling": data["download"]["collision"]["collision_handling"],
    }
    expected_manifest = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    if args.check:
        if (root / FIXTURE).read_bytes() != content:
            raise SystemExit("file-guard Go fixture is stale")
        if (root / MANIFEST).read_bytes() != expected_manifest:
            raise SystemExit("file-guard manifest is stale")
    else:
        (root / FIXTURE).write_bytes(content)
        (root / MANIFEST).write_bytes(expected_manifest)
    print(f"file-guard fixture passed ({len(data['upload'])} upload cases)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
