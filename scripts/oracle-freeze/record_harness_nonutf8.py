#!/usr/bin/env python3
"""Historical recorder: Go `symbrain harness list` with non-UTF-8 HOME / environment bytes.

REQUIRES a Go-built `symbrain-go` binary; NOT part of any gate (deleted with the Go sources).
Writes rust/symbrain-cli/tests/fixtures/harness_nonutf8_go.json, which the Rust test
`harness_nonutf8` replays without Go.

    record_harness_nonutf8.py <symbrain-go> <out.json> <go-version-string> <worktree-commit>
"""
from __future__ import annotations

import json
import os
import platform
import subprocess
import sys
import tempfile

ROOT_TOKEN = b"<ROOT>"
SUFFIXES = [b"/missing-\xff\xfe", b"/missing-\xe2\x82"]
ARGVS = [
    ["harness", "list"],
    ["harness", "list", "--json"],
    ["install", "--harness", "claude", "--profile", "test", "--dry-run"],
    ["uninstall", "--harness", "claude", "--dry-run"],
]


def main() -> int:
    binary, out, go_version, commit = sys.argv[1:5]
    cases = []
    with tempfile.TemporaryDirectory(prefix="nonutf8.") as raw_root:
        root = os.path.realpath(raw_root)
        for suffix in SUFFIXES:
            home = os.fsencode(root) + suffix
            for argv in ARGVS:
                env = {
                    b"HOME": home,
                    b"UNRELATED": b"\xff",
                    b"\xfe": b"invalid-name",
                }
                proc = subprocess.run([binary, *argv], env=env, capture_output=True, check=False)
                cases.append({
                    "suffix_hex": suffix.hex(),
                    "argv": argv,
                    "exit": proc.returncode,
                    "stdout_hex": proc.stdout.replace(os.fsencode(root), ROOT_TOKEN).hex(),
                    "stderr_hex": proc.stderr.replace(os.fsencode(root), ROOT_TOKEN).hex(),
                })
    document = {
        "schema_version": 1,
        "root_token": ROOT_TOKEN.decode(),
        "provenance": {
            "recorder": "scripts/oracle-freeze/record_harness_nonutf8.py",
            "command": [os.path.basename(binary), "<argv per case>"],
            "go_version": go_version,
            "worktree_commit": commit,
            "platform": f"{platform.system().lower()}/{platform.machine()}",
            "env": "env_clear; HOME=<root><suffix>; UNRELATED=0xff; env name 0xfe=invalid-name",
        },
        "cases": cases,
    }
    with open(out, "w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=1)
        handle.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
