"""Retain actual native Windows executable bytes before their owned runner exits."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(65536):
            value.update(chunk)
    return value.hexdigest()


def pe_machine(path):
    with path.open("rb") as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise ValueError("actual Windows PE executable required")
        offset = int.from_bytes(header[60:64], "little")
        if offset < 64 or offset > path.stat().st_size - 24:
            raise ValueError("PE header offset outside retained executable")
        stream.seek(offset)
        pe = stream.read(24)
        if pe[:4] != b"PE\0\0":
            raise ValueError("missing actual PE signature")
        return int.from_bytes(pe[4:6], "little")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", nargs=2, action="append", required=True, metavar=("ROLE", "PATH"))
    args = parser.parse_args()
    if os.name != "nt":
        raise ValueError("native Windows collection required; no portable runtime claim")
    os.umask(0o022)
    output = args.output.resolve()
    files = Path(str(output) + ".files")
    if output.exists() or files.exists() or output.is_relative_to(ROOT):
        raise ValueError("new owned outputs outside source checkout required")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT):
        raise ValueError("native candidate source must be clean")
    report = {"candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
              "native_windows": True, "collector_sha256": digest(Path(__file__)), "binaries": []}
    roles = set()
    files.mkdir(parents=True, exist_ok=False)
    for role, name in args.binary:
        if not re.fullmatch(r"[a-z][a-z0-9-]*", role) or role in roles:
            raise ValueError("distinct bounded native executable role names required")
        roles.add(role)
        source = Path(name).resolve()
        sha = digest(source)
        machine = pe_machine(source)
        copy = files / (role + ".exe")
        shutil.copy2(source, copy)
        if digest(copy) != sha or digest(source) != sha:
            raise ValueError("actual native executable changed during retention")
        report["binaries"].append({"role": role, "source": str(source), "retained": str(copy),
                                  "sha256": sha, "bytes": copy.stat().st_size, "pe_machine": machine})
        output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
