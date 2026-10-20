#!/usr/bin/env python3
"""Build the release archives for the Rust CLI and its Go fallback.

  package.py archive --version V --os linux --arch amd64 \
      --rust target/release/symbrain --go /tmp/symbrain-go --out dist
  package.py checksums --dir dist

Archive names and documentation members follow `.goreleaser.yml`
(`symbrain_<version>_<os>_<arch>.tar.gz`, `.zip` on Windows). Until the Go
fallback is removed (#771), each archive also carries `symbrain-go`, which the
Rust CLI finds on PATH next to itself. Output is byte-for-byte reproducible for
the same inputs and SOURCE_DATE_EPOCH.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import os
import re
import sys
import tarfile
import time
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOCS = ["AGENTS.md", "LICENSE", "README.md"]
PLATFORMS = [(goos, goarch) for goos in ["darwin", "linux", "windows"] for goarch in ["amd64", "arm64"]]
VERSION = re.compile(r"^[0-9A-Za-z.+-]+$")
ZIP_EPOCH = 315532800  # 1980-01-01, the earliest ZIP timestamp


def archive_name(version: str, goos: str, goarch: str) -> str:
    return f"symbrain_{version}_{goos}_{goarch}." + ("zip" if goos == "windows" else "tar.gz")


def members(goos: str, rust: Path, go: Path) -> list[tuple[str, bytes, int]]:
    exe = ".exe" if goos == "windows" else ""
    entries = [(name, (ROOT / name).read_bytes(), 0o644) for name in DOCS]
    entries += [("symbrain" + exe, rust.read_bytes(), 0o755), ("symbrain-go" + exe, go.read_bytes(), 0o755)]
    return sorted(entries)


def write_archive(path: Path, entries: list[tuple[str, bytes, int]], epoch: int) -> None:
    if path.suffix == ".zip":
        stamp = time.gmtime(max(epoch, ZIP_EPOCH))[:6]
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
            for name, data, mode in entries:
                info = zipfile.ZipInfo(name, stamp)
                info.create_system = 3  # Unix, so the mode bits below are honoured
                info.external_attr = (0o100000 | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, data)
        return
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.PAX_FORMAT) as archive:
        for name, data, mode in entries:
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), mode, epoch
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            archive.addfile(info, io.BytesIO(data))
    with open(path, "wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as gz:
        gz.write(buffer.getvalue())


def write_checksums(directory: Path) -> Path:
    archives = sorted(p for p in directory.iterdir() if p.name.endswith((".tar.gz", ".zip")))
    if not archives:
        raise SystemExit(f"no archives in {directory}")
    lines = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in archives]
    target = directory / "checksums.txt"
    target.write_text("".join(lines), encoding="ascii", newline="\n")
    return target


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("archive")
    build.add_argument("--version", required=True)
    build.add_argument("--os", required=True, choices=sorted({p[0] for p in PLATFORMS}))
    build.add_argument("--arch", required=True, choices=sorted({p[1] for p in PLATFORMS}))
    build.add_argument("--rust", required=True, type=Path)
    build.add_argument("--go", required=True, type=Path)
    build.add_argument("--out", required=True, type=Path)
    sums = commands.add_parser("checksums")
    sums.add_argument("--dir", required=True, type=Path)
    args = parser.parse_args(argv)

    if args.command == "checksums":
        print(write_checksums(args.dir))
        return 0
    if not VERSION.fullmatch(args.version) or args.version.startswith("v"):
        raise SystemExit(f"invalid version {args.version!r} (no leading v)")
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    args.out.mkdir(parents=True, exist_ok=True)
    path = args.out / archive_name(args.version, args.os, args.arch)
    write_archive(path, members(args.os, args.rust, args.go), epoch)
    print(path)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
