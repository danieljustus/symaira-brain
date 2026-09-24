#!/usr/bin/env python3
"""Exercise an unsigned dual candidate locally; never publish or install it."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path

import verify


def _checked_archive(dual: Path, implementation: str, name: str) -> Path | None:
    directory = dual / implementation
    archive = directory / name
    if not archive.is_file():
        return None
    checksums = verify._read_checksum_manifest(directory / "checksums.txt")
    if checksums.get(name) != verify._sha256(archive):
        raise verify.GateError(f"{implementation} archive integrity failure: {name}")
    return archive


def select_archive(candidate: Path, version: str, target: str, requested: str | None) -> tuple[str, Path]:
    if target not in {f"{os_name}-{arch}" for os_name, arch in verify.TARGETS}:
        raise verify.GateError(f"unsupported native target: {target}")
    dual = candidate / "dual" if (candidate / "dual").is_dir() else candidate
    verify.validate_selection_contract(verify._read_json(dual / "dual-release-manifest.json", "dual release manifest"))
    os_name, arch = target.split("-", 1)
    name = verify.archive_name(version, os_name, arch)
    go = dual / "go" / name
    rust = dual / "rust" / name
    rust_checked = _checked_archive(dual, "rust", name) if rust.is_file() and requested == "rust" else None
    selected = verify.select_implementation(
        requested,
        go_available=go.is_file(),
        rust_available=rust.is_file(),
        rust_integrity_ok=rust_checked is not None or not rust.is_file(),
    )
    archive = _checked_archive(dual, selected, name)
    if archive is None:
        raise verify.GateError(f"selected {selected} archive is unavailable: {name}")
    return selected, archive


def _extract_binary(archive: Path, destination: Path, binary_name: str) -> Path:
    verify._read_archive_members(archive)
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as source:
            members = [item for item in source.infolist() if not item.is_dir() and Path(item.filename).name == binary_name]
            if len(members) != 1:
                raise verify.GateError(f"{archive.name} must contain exactly one {binary_name}")
            data = source.read(members[0])
    else:
        with tarfile.open(archive, "r:gz") as source:
            members = [item for item in source.getmembers() if item.isfile() and Path(item.name).name == binary_name]
            if len(members) != 1:
                raise verify.GateError(f"{archive.name} must contain exactly one {binary_name}")
            stream = source.extractfile(members[0])
            if stream is None:
                raise verify.GateError(f"cannot read {binary_name} from {archive.name}")
            data = stream.read()
    binary = destination / binary_name
    binary.write_bytes(data)
    if os.name != "nt":
        binary.chmod(0o700)
    return binary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("args", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    try:
        selected, archive = select_archive(args.candidate, args.version, args.target, os.environ.get("SYMBROWSE_IMPL"))
        if args.dry_run:
            print(json.dumps({"selected": selected, "archive": archive.name}, sort_keys=True))
            return 0
        binary_name = "symbrowse.exe" if args.target.startswith("windows-") else "symbrowse"
        with tempfile.TemporaryDirectory(prefix="symbrowse-candidate-") as raw:
            binary = _extract_binary(archive, Path(raw), binary_name)
            command_args = args.args[1:] if args.args[:1] == ["--"] else args.args
            return subprocess.run([str(binary), *command_args], check=False).returncode
    except (OSError, verify.GateError) as error:
        parser.exit(1, f"candidate launcher: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
