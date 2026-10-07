#!/usr/bin/env python3
"""Prepare the checksum-pinned official SQLite SDK for Linux checker Python only."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import zipfile

from provider import C_SHA3_256, SOURCE_ID, VERSION, ZIP_SHA256


def verified_sources(archive):
    data = archive.read_bytes()
    assert hashlib.sha256(data).hexdigest() == ZIP_SHA256, "official SDK ZIP mismatch"
    with zipfile.ZipFile(archive) as bundle:
        names = {name for name in bundle.namelist() if not name.endswith("/")}
        expected = {"sqlite-amalgamation-3500400/" + name
                    for name in ["sqlite3.c", "sqlite3.h", "sqlite3ext.h", "shell.c"]}
        assert names == expected, "unexpected official SDK members"
        payloads = {Path(name).name: bundle.read(name) for name in sorted(names)}
    assert hashlib.sha3_256(payloads["sqlite3.c"]).hexdigest() == C_SHA3_256
    assert ('#define SQLITE_SOURCE_ID      "' + SOURCE_ID + '"').encode() in payloads["sqlite3.h"]
    return payloads


def build(archive, output):
    assert sys.platform == "linux", "only the pinned Linux extension dynamically links SQLite"
    sources = verified_sources(archive)
    output.mkdir(parents=True, exist_ok=False)
    output = output.resolve(strict=True)
    source = output / "sqlite3.c"
    for name, data in sources.items():
        (output / name).write_bytes(data)
    result = dict(status="preparing", platform=sys.platform, sqlite_version=VERSION,
                  sqlite_source_id=SOURCE_ID, zip_sha256=ZIP_SHA256, c_sha3_256=C_SHA3_256,
                  archive_path=str(archive.resolve()), output=str(output), commands=[])
    report = output / "provider.json"

    def save():
        report.write_bytes((json.dumps(result, indent=2, sort_keys=True) + "\n").encode())

    def run(argv):
        entry = dict(argv=argv, timeout_seconds=120, status="started")
        result["commands"].append(entry)
        save()
        try:
            process = subprocess.run(argv, cwd=output, capture_output=True, timeout=120, check=False)
        except subprocess.TimeoutExpired as error:
            entry.update(status="timeout", stdout_hex=(error.stdout or b"").hex(),
                         stderr_hex=(error.stderr or b"").hex())
            save()
            raise
        entry.update(status="finished", exit=process.returncode,
                     stdout_hex=process.stdout.hex(), stderr_hex=process.stderr.hex())
        save()
        assert process.returncode == 0, entry
        return process.stdout

    save()
    try:
        run(["cc", "--version"])
        run(["cc", "-O2", "-fPIC", "-shared", "-DSQLITE_THREADSAFE=1",
             "-DSQLITE_ENABLE_FTS5", "-DSQLITE_ENABLE_COLUMN_METADATA",
             "-Wl,-soname,libsqlite3.so.0", str(source), "-o", str(output / "libsqlite3.so.0"),
             "-lm", "-ldl", "-lpthread"])
        dynamic = run(["readelf", "-d", str(output / "libsqlite3.so.0")])
        assert b"(SONAME)" in dynamic and b"[libsqlite3.so.0]" in dynamic, dynamic
        result.update(status="built", library_sha256=hashlib.sha256(
            (output / "libsqlite3.so.0").read_bytes()).hexdigest(),
            semantic_preflight="required before any historical product child")
        save()
    except BaseException as error:
        result.update(status="failed", error_type=type(error).__name__, error=str(error))
        save()
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o022)
    build(args.archive, args.output)


if __name__ == "__main__":
    main()
