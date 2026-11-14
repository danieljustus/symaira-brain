#!/usr/bin/env python3
"""Build and verify the release archives of the Rust CLI.

  package.py archive --version V --os linux --arch amd64 \
      --rust target/release/symbrain --out dist
  package.py archive --version V --os linux --arch amd64 --product symbrowse \
      --rust browse/target/release/symbrowse --out dist
  package.py checksums --dir dist
  package.py verify --version V --dir dist

Archive names and documentation members keep the GoReleaser layout of the Go
releases (`<product>_<version>_<os>_<arch>.tar.gz`, product `symbrain` or
`symbrowse`). Each archive holds only its product binary and the documentation
(the repository-root AGENTS.md, LICENSE and README.md for every product); there
is no Go fallback. `verify` requires the full matrix: every product on every
platform. Output is byte-for-byte reproducible for the same inputs and
SOURCE_DATE_EPOCH.
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
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOCS = ["AGENTS.md", "LICENSE", "README.md"]
PLATFORMS = [("darwin", "arm64"), ("linux", "amd64"), ("linux", "arm64")]
PRODUCTS = ["symbrain", "symbrowse"]
VERSION = re.compile(r"^[0-9A-Za-z.+-]+$")


def archive_name(version: str, goos: str, goarch: str, product: str = "symbrain") -> str:
    return f"{product}_{version}_{goos}_{goarch}.tar.gz"


def members(rust: Path, product: str = "symbrain") -> list[tuple[str, bytes, int]]:
    entries = [(name, (ROOT / name).read_bytes(), 0o644) for name in DOCS]
    entries.append((product, rust.read_bytes(), 0o755))
    return sorted(entries)


def write_archive(path: Path, entries: list[tuple[str, bytes, int]], epoch: int) -> None:
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
    archives = sorted(p for p in directory.iterdir() if p.name.endswith(".tar.gz"))
    if not archives:
        raise SystemExit(f"no archives in {directory}")
    lines = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in archives]
    target = directory / "checksums.txt"
    target.write_text("".join(lines), encoding="ascii", newline="\n")
    return target


def verify(version: str, directory: Path) -> list[str]:
    """Return every release-contract violation of a complete bundle."""
    problems = []
    expected = {
        archive_name(version, goos, goarch, product) for product in PRODUCTS for goos, goarch in PLATFORMS
    }
    present = {p.name for p in directory.iterdir() if p.name.endswith(".tar.gz")}
    problems += [f"missing archive {name}" for name in sorted(expected - present)]
    problems += [f"unexpected archive {name}" for name in sorted(present - expected)]
    sums = directory / "checksums.txt"
    listed = {}
    if sums.is_file():
        for line in sums.read_text(encoding="ascii").splitlines():
            digest, _, name = line.partition("  ")
            listed[name] = digest
    else:
        problems.append("missing checksums.txt")
    if sums.is_file() and set(listed) != present:
        problems.append(f"checksums.txt lists {sorted(listed)}, bundle has {sorted(present)}")
    docs = {name: (ROOT / name).read_bytes() for name in DOCS}
    for name in sorted(expected & present):
        product = name.split("_", 1)[0]
        path = directory / name
        if listed.get(name) != hashlib.sha256(path.read_bytes()).hexdigest():
            problems.append(f"{name}: checksum mismatch")
        with tarfile.open(path) as archive:
            entries = {m.name: m for m in archive.getmembers()}
            if sorted(entries) != sorted([*DOCS, product]):
                problems.append(f"{name}: members {sorted(entries)}")
                continue
            for member, info in entries.items():
                mode = 0o755 if member == product else 0o644
                if not info.isfile() or info.mode != mode or (info.uid, info.gid) != (0, 0):
                    problems.append(f"{name}: {member} is not a root-owned {oct(mode)} file")
                data = archive.extractfile(info).read()
                if member in docs and data != docs[member]:
                    problems.append(f"{name}: {member} differs from the source tree")
                if member == product and not data:
                    problems.append(f"{name}: empty {product} binary")
    return problems


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("archive")
    build.add_argument("--version", required=True)
    build.add_argument("--os", required=True, choices=sorted({p[0] for p in PLATFORMS}))
    build.add_argument("--arch", required=True, choices=sorted({p[1] for p in PLATFORMS}))
    build.add_argument("--product", default="symbrain", choices=PRODUCTS)
    build.add_argument("--rust", required=True, type=Path, help="path of the built product binary")
    build.add_argument("--out", required=True, type=Path)
    sums = commands.add_parser("checksums")
    sums.add_argument("--dir", required=True, type=Path)
    check = commands.add_parser("verify")
    check.add_argument("--version", required=True)
    check.add_argument("--dir", required=True, type=Path)
    args = parser.parse_args(argv)

    if args.command == "checksums":
        print(write_checksums(args.dir))
        return 0
    if args.command == "verify":
        problems = verify(args.version, args.dir)
        for problem in problems:
            print(f"release contract: FAIL {problem}", file=sys.stderr)
        if problems:
            return 1
        print(f"release contract: PASS {len(PRODUCTS) * len(PLATFORMS)} archives for {args.version}")
        return 0
    if not VERSION.fullmatch(args.version) or args.version.startswith("v"):
        raise SystemExit(f"invalid version {args.version!r} (no leading v)")
    if (args.os, args.arch) not in PLATFORMS:
        raise SystemExit(f"unsupported release platform: {args.os}/{args.arch}")
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    args.out.mkdir(parents=True, exist_ok=True)
    path = args.out / archive_name(args.version, args.os, args.arch, args.product)
    write_archive(path, members(args.rust, args.product), epoch)
    print(path)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
