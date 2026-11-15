#!/usr/bin/env python3
"""Tests for the RUST-017 native browser-transport verifier."""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import browser_transport_native as gate  # noqa: E402


def passing(cases: tuple[str, ...]) -> str:
    return "".join(f"PARITY_CASE {case} PASS\n" for case in cases)


class VerifyTests(unittest.TestCase):
    def test_full_run_is_accepted(self) -> None:
        self.assertEqual(len(gate.verify(passing(gate.ADAPTER_CASES), gate.ADAPTER_CASES)), len(gate.ADAPTER_CASES))

    def test_libtest_prefix_is_counted(self) -> None:
        output = "test corpus_chrome ... " + passing(gate.ADAPTER_CASES)
        gate.verify(output, gate.ADAPTER_CASES)

    def test_missing_unexpected_and_duplicate_cases_are_rejected(self) -> None:
        output = passing(gate.ADAPTER_CASES)
        with self.assertRaisesRegex(AssertionError, "missing"):
            gate.verify(output.replace("PARITY_CASE ENG-010.cleanup PASS\n", ""), gate.ADAPTER_CASES)
        with self.assertRaisesRegex(AssertionError, "unexpected"):
            gate.verify(output + "PARITY_CASE ENG-010.made-up PASS\n", gate.ADAPTER_CASES)
        with self.assertRaisesRegex(AssertionError, "duplicates"):
            gate.verify(output + "PARITY_CASE ENG-010.cleanup PASS\n", gate.ADAPTER_CASES)

    def test_an_unreported_run_is_not_evidence(self) -> None:
        with self.assertRaisesRegex(AssertionError, "missing"):
            gate.verify("test result: ok. 1 passed\n", gate.DAEMON_CASES)

    def test_safari_must_pass_or_skip_with_a_reason(self) -> None:
        base = passing(gate.DAEMON_CASES)
        passed = gate.verify(base + passing((gate.SAFARI_PASS,)), gate.DAEMON_CASES, safari=True)
        self.assertEqual(passed[gate.SAFARI_PASS], "PASS")
        skipped = gate.verify(
            base + "PARITY_CASE ENG-009.safari SKIPPED normal Safari is running\n",
            gate.DAEMON_CASES,
            safari=True,
        )
        self.assertTrue(skipped[gate.SAFARI_SKIP].startswith("SKIPPED: normal Safari"))
        with self.assertRaisesRegex(AssertionError, "exactly once"):
            gate.verify(base, gate.DAEMON_CASES, safari=True)
        with self.assertRaisesRegex(AssertionError, "exactly once"):
            gate.verify(
                base + passing((gate.SAFARI_PASS,)) + "PARITY_CASE ENG-009.safari SKIPPED x\n",
                gate.DAEMON_CASES,
                safari=True,
            )

    def test_skips_are_rejected_for_non_safari_runs(self) -> None:
        with self.assertRaisesRegex(AssertionError, "skipped"):
            gate.verify(passing(gate.ADAPTER_CASES) + "PARITY_CASE ENG-010.cookies SKIPPED no\n", gate.ADAPTER_CASES)


if __name__ == "__main__":
    unittest.main()
