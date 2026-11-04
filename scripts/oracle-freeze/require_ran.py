#!/usr/bin/env python3
"""Run a test command and fail unless it actually executed enough test cases.

A green exit with zero matching tests (a filter typo, a renamed test, an
`#[ignore]`) is not evidence. This wrapper tees the command's output and requires
every libtest "test result: ok. N passed" summary to be present and the total
number of passed tests to reach --min, with no failures and no ignored tests
unless --allow-ignored is given.
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys

RESULT = re.compile(r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;", re.M)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--min", type=int, default=1, help="minimum total passed tests")
    parser.add_argument("--allow-ignored", action="store_true")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command and args.command[0] == "--" else args.command
    if not command:
        parser.error("missing command")
    proc = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace")
    sys.stdout.write(proc.stdout)
    if proc.returncode != 0:
        print(f"require_ran: command exited {proc.returncode}", file=sys.stderr)
        return proc.returncode
    results = RESULT.findall(proc.stdout)
    passed = sum(int(p) for _, p, _, _ in results)
    failed = sum(int(f) for _, _, f, _ in results)
    ignored = sum(int(i) for _, _, _, i in results)
    if failed or any(status != "ok" for status, *_ in results):
        print("require_ran: failing test summary", file=sys.stderr)
        return 1
    if passed < args.min:
        print(f"require_ran: only {passed} test(s) passed, need at least {args.min} (zero-case run is not a pass)", file=sys.stderr)
        return 1
    if ignored and not args.allow_ignored:
        print(f"require_ran: {ignored} ignored test(s) in a gate run", file=sys.stderr)
        return 1
    print(f"require_ran: {passed} test(s) passed, {ignored} ignored")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
