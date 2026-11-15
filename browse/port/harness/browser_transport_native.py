#!/usr/bin/env python3
"""RUST-017 native browser-transport gate (ENG-009 / ENG-010).

Runs the shared engine corpus against every explicitly requested local engine
(Chrome, Firefox, and Safari only when the user's normal Safari is not
running), then verifies from the captured output that every expected case
executed. A skipped, partial or duplicated run is never evidence; the one
tolerated skip is the guarded Safari case, which must print its reason and is
recorded as skipped-with-reason in the machine-readable evidence file.
"""
from __future__ import annotations

import json
import platform
import re
import subprocess
import sys
from pathlib import Path

CASE_RE = re.compile(r"^(?:test \S+ \.\.\. )?PARITY_CASE (\S+) PASS$", re.MULTILINE)
SKIP_RE = re.compile(r"^(?:test \S+ \.\.\. )?PARITY_CASE (\S+) SKIPPED (.+)$", re.MULTILINE)

# Keep in sync with `case(...)` in crates/symbrowse-daemon/tests/engine_native_corpus.rs
# and the println! cases in crates/symbrowse-engine-firefox/tests/native.rs.
CORPUS_STEPS = (
    "selected-engine-version",
    "redirect",
    "javascript",
    "cookies",
    "storage",
    "response-capture",
    "capability-truth",
    "timeout",
    "cleanup",
)
DAEMON_CASES = tuple(
    f"ENG-009.{step}.{engine}" for engine in ("chrome", "firefox") for step in CORPUS_STEPS
) + (
    "ENG-009.unavailable-typed.chrome",
    "ENG-009.unavailable-typed.firefox",
)
ADAPTER_CASES = (
    "ENG-010.lifecycle-version",
    "ENG-010.navigation-javascript",
    "ENG-010.cookies",
    "ENG-010.storage",
    "ENG-010.response-capture",
    "ENG-010.typed-unsupported",
    "ENG-010.timeout",
    "ENG-010.cleanup",
)
SAFARI_INFO = "ENG-009.safari-info.safari-bidi"  # macOS only: version read without launching Safari
SAFARI_PASS = "ENG-009.safari-redirect.safari-bidi"
SAFARI_SKIP = "ENG-009.safari"


def verify(output: str, expected: tuple[str, ...], *, safari: bool = False) -> dict[str, str]:
    """Return `{case: PASS|SKIPPED: reason}` or raise when the run is not evidence."""
    executed = CASE_RE.findall(output)
    skipped = dict(SKIP_RE.findall(output))
    allowed = set(expected) | ({SAFARI_PASS} if safari else set())
    duplicates = sorted({case for case in executed if executed.count(case) > 1})
    missing = [case for case in expected if case not in executed]
    unexpected = [case for case in executed if case not in allowed]
    if duplicates or missing or unexpected:
        raise AssertionError(
            f"native cases mismatch: missing={missing} unexpected={unexpected} duplicates={duplicates}"
        )
    results = {case: "PASS" for case in executed}
    if safari:
        passed = SAFARI_PASS in executed
        if passed == (SAFARI_SKIP in skipped):
            raise AssertionError("Safari must either pass or be skipped with a reason, exactly once")
        if not passed:
            if not skipped[SAFARI_SKIP].strip():
                raise AssertionError("a skipped Safari case must record its reason")
            results[SAFARI_SKIP] = f"SKIPPED: {skipped[SAFARI_SKIP].strip()}"
    elif skipped:
        raise AssertionError(f"unexpected skipped cases: {sorted(skipped)}")
    return results


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


def browser_transport_native(root: Path, env: dict[str, str]) -> None:
    env = dict(env, SYMBROWSE_E2E="1")
    adapter = _capture(
        ["cargo", "test", "-p", "symbrowse-engine-firefox", "--test", "native", "--locked", "--", "--nocapture", "--test-threads=1"],
        root,
        env,
    )
    daemon = _capture(
        ["cargo", "test", "-p", "symbrowse-daemon", "--test", "engine_native_corpus", "--locked", "--", "--nocapture", "--test-threads=1"],
        root,
        env,
    )
    macos = platform.system() == "Darwin"
    results = {
        **verify(adapter, ADAPTER_CASES),
        **verify(daemon, DAEMON_CASES + ((SAFARI_INFO,) if macos else ()), safari=macos),
    }
    evidence = {
        "schema_version": 1,
        "suite": "browser-transport",
        "platform": platform.platform(),
        "requested": True,
        "cases": results,
        "safari": results.get(SAFARI_SKIP, results.get(SAFARI_PASS, "not-applicable")),
    }
    path = root / "target" / "native-evidence" / "browser-transport.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"native evidence: {path}")
    print(f"browser-transport native gate passed: {len(results)} cases", flush=True)
