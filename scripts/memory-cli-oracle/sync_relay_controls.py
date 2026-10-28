#!/usr/bin/env python3
"""Real-process negative controls for the `sync_relay` family (Unix only).

Each mutant wraps the actual native CLI and perturbs exactly one observable
aspect of a native-kind scenario, after the real process has run:

* ``stdout`` / ``stderr`` / ``exit``: one byte or the failing status changes;
* ``request``: the wrapper first sends one extra request to the relay, so the
  relay records a request Go never sent;
* ``database``: the wrapper stores an extra cursor row in the database.

The family replay must reject every mutant, and every rejected native record
must differ from the immutable Go process through exactly the mutated field
(``stdout``, ``stderr``, ``exit``, ``requests`` or ``state``). Hand-off records
only assert that work stays on Go, so they take no part in the attribution.
"""
import argparse
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile

# Replay children run with an empty PATH, so wrappers use absolute tools only.
PYTHON = sys.executable

SHELL_MUTANTS = {
    "stdout": "printf 'mutant'",
    "stderr": "printf ' mutant' >&2",
    # every native observation exits 0 or 1: swap the two
    "exit": 'if [ "$code" -eq 0 ]; then code=1; else code=0; fi',
}

EXTRA_REQUEST = r'''
import socket, sys
remote = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--remote=http://127.0.0.1:")), None)
if remote and ":" in remote.rsplit("/", 1)[-1]:
    port = int(remote.split(":")[2].split("/")[0])
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=2) as s:
            s.sendall(b"GET /extra HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            s.recv(65536)
    except OSError:
        pass
'''

REWRITE_CURSOR = r'''
import sqlite3, sys
args = sys.argv[1:]
if "--db" in args:
    path = args[args.index("--db") + 1]
    try:
        with sqlite3.connect(path) as db:
            db.execute("INSERT OR REPLACE INTO sync_state(remote, last_sync) VALUES('mutant', '1999-01-01 00:00:00 +0000 UTC')")
    except sqlite3.Error:
        pass
'''


def write_wrapper(path, rust, name):
    if name in SHELL_MUTANTS:
        path.write_text(f'#!/bin/sh\n"{rust}" "$@"\ncode=$?\n{SHELL_MUTANTS[name]}\nexit $code\n')
    elif name == "request":
        helper = path.with_name("extra_request.py")
        helper.write_text(EXTRA_REQUEST)
        path.write_text(f'#!/bin/sh\n"{PYTHON}" -I "{helper}" "$@"\nexec "{rust}" "$@"\n')
    else:
        helper = path.with_name("rewrite_cursor.py")
        helper.write_text(REWRITE_CURSOR)
        path.write_text(f'#!/bin/sh\n"{rust}" "$@"\ncode=$?\n"{PYTHON}" -I "{helper}" "$@"\nexit $code\n')
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


# The one observation key each mutant disturbs.
ATTRIBUTION = {"stdout": "stdout", "stderr": "stderr", "exit": "exit", "request": "requests",
               "database": "state"}


def differing(record):
    keys = set()
    go, rust = record["go"], record["rust"]
    for first, second in zip(go["steps"], rust["steps"]):
        for key in ("exit", "stdout", "stderr", "requests"):
            if first[key] != second[key]:
                keys.add(key)
    if go["state"] != rust["state"]:
        keys.add("state")
    return keys


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust", type=Path)
    parser.add_argument("--go", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if os.name != "posix":
        print(json.dumps({"sync_relay_controls_skipped": "posix only"}))
        return
    repo = Path(__file__).resolve().parents[2]
    rust = (args.rust or repo / "target/debug/symbrain").resolve()
    results = []
    with tempfile.TemporaryDirectory(prefix="memory-cli-relay-control-") as temporary:
        root = Path(temporary)
        for name in ("stdout", "stderr", "exit", "request", "database"):
            wrapper = root / f"mutant-{name}"
            write_wrapper(wrapper, rust, name)
            report = root / f"{name}.json"
            command = [sys.executable, str(repo / "scripts/memory-cli-oracle/replay.py"),
                       "--rust", str(wrapper), "--family", "sync_relay", "--report", str(report)]
            if args.go:
                command.extend(("--go", str(args.go.resolve())))
            child = subprocess.run(command, cwd=repo, capture_output=True)
            receipt = json.loads(report.read_bytes())
            assert child.returncode != 0, "negative control unexpectedly passed: " + name
            assert receipt["cases"] > 200 and receipt["passed"] < receipt["cases"], (name, receipt["cases"], receipt["passed"])
            assert b"actual Go/Rust output or database state differs" in child.stderr, child.stderr
            rejected = [record for record in receipt["records"] if not record["match"] and record["kind"] == "native"]
            assert rejected, name
            for record in rejected:
                assert differing(record) == {ATTRIBUTION[name]}, (name, record["name"], differing(record))
            results.append(dict(mutant=name, exit=child.returncode, cases=receipt["cases"],
                                rejected_native_cases=len(rejected)))
    if args.report:
        args.report.write_text(json.dumps(dict(controls=results), indent=2) + "\n")
    print(json.dumps({"sync_relay_controls_rejected": len(results), "detail": results}))


if __name__ == "__main__":
    main()
