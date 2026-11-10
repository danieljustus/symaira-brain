#!/usr/bin/env python3
"""RUST-012 daemon-path Chrome suite (ENG-005/006/007).

Runs the production-daemon Chrome test and the frozen Go-oracle replay against
a real, isolated Chrome, then verifies from the captured output that every
expected parity case executed. A skipped run, a zero-case run or a missing case
is a failure: none of them is evidence.
"""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

# Keep in sync with `case(...)` in crates/symbrowse-daemon/tests/chrome_native.rs.
EXPECTED_CASES = (
    "ENG-005.navigate-read",
    "ENG-005.interactions",
    "ENG-005.form-input",
    "ENG-005.inspect-find-wait",
    "ENG-006.tabs",
    "ENG-006.frames",
    "ENG-006.dialogs-manual",
    "ENG-006.dialogs-auto",
    "ENG-007.network-capture",
    "ENG-007.a11y",
    "ENG-007.network-offline-block",
    "ENG-007.network-har",
    "ENG-007.screenshot",
    "ENG-007.pdf",
    "ENG-007.upload",
    "ENG-007.download",
    "ENG-007.unsupported-explicit",
)

# Go steps the Rust daemon documents as not reproduced (see oracle_replay.rs).
EXPECTED_KNOWN_GAPS = 1

CASE_RE = re.compile(r"^PARITY_CASE (\S+) PASS$", re.MULTILINE)
REPLAY_RE = re.compile(
    r"oracle replay: (\d+) matched, (\d+) mismatched \((\d+) known gaps\) of (\d+) steps"
)


def parse_cases(output: str) -> list[str]:
    return CASE_RE.findall(output)


def verify_cases(output: str) -> None:
    executed = parse_cases(output)
    duplicates = sorted({case for case in executed if executed.count(case) > 1})
    missing = [case for case in EXPECTED_CASES if case not in executed]
    unexpected = [case for case in executed if case not in EXPECTED_CASES]
    if duplicates or missing or unexpected:
        raise AssertionError(
            f"daemon parity cases mismatch: missing={missing} unexpected={unexpected} duplicates={duplicates}"
        )
    if "test result: ok. 1 passed; 0 failed; 0 ignored" not in output:
        raise AssertionError("chrome_native did not report exactly one passed test (skipped runs are not evidence)")


def verify_replay(output: str, scenario_steps: int) -> tuple[int, int]:
    match = REPLAY_RE.search(output)
    if match is None:
        raise AssertionError("oracle replay did not run (SKIP is not evidence)")
    matched, mismatched, known, steps = map(int, match.groups())
    if steps != scenario_steps or matched + mismatched != steps:
        raise AssertionError(f"oracle replay covered {matched}+{mismatched} of {steps} steps, expected {scenario_steps}")
    if mismatched != known or known != EXPECTED_KNOWN_GAPS:
        raise AssertionError(f"oracle replay has {mismatched} mismatches / {known} known gaps, expected {EXPECTED_KNOWN_GAPS}")
    return matched, steps


def _capture(command: list[str], root: Path, env: dict[str, str]) -> str:
    print("+", " ".join(command), flush=True)
    result = subprocess.run(
        command, cwd=root, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=1500
    )
    sys.stdout.write(result.stdout)
    sys.stdout.flush()
    if result.returncode != 0:
        raise AssertionError(f"{' '.join(command)} exited {result.returncode}")
    return result.stdout


def chrome_daemon_suite(root: Path, env: dict[str, str]) -> None:
    import json

    if env.get("SYMBROWSE_E2E") != "1":
        raise AssertionError("chrome-daemon needs SYMBROWSE_E2E=1 and a real Chrome; there is no skip mode")
    scenarios = json.loads(
        (root / "crates/symbrowse-daemon/tests/fixtures/daemon_oracle/scenarios_chrome.json").read_text()
    )
    native = _capture(
        ["cargo", "test", "-p", "symbrowse-daemon", "--test", "chrome_native", "--locked", "--", "--nocapture", "--test-threads=1"],
        root,
        env,
    )
    verify_cases(native)
    replay = _capture(
        ["cargo", "test", "-p", "symbrowse-daemon", "--test", "oracle_replay", "--locked", "--", "--nocapture", "--test-threads=1"],
        root,
        dict(env, SYMBROWSE_LIVE_CHROME="1"),
    )
    matched, steps = verify_replay(replay, len(scenarios["steps"]))
    print(
        f"chrome-daemon suite passed: {len(EXPECTED_CASES)} daemon parity cases, "
        f"{matched}/{steps} frozen Go-oracle steps matched ({EXPECTED_KNOWN_GAPS} documented gap)",
        flush=True,
    )
