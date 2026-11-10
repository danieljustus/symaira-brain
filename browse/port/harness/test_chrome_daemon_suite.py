#!/usr/bin/env python3
"""Tests for the RUST-012 daemon-path Chrome suite verifier."""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import chrome_daemon_suite as suite  # noqa: E402


def passing_output() -> str:
    lines = [f"PARITY_CASE {case} PASS" for case in suite.EXPECTED_CASES]
    return "\n".join(lines) + "\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"


class CaseVerificationTests(unittest.TestCase):
    def test_full_run_is_accepted(self) -> None:
        suite.verify_cases(passing_output())

    def test_first_case_after_the_libtest_prefix_is_counted(self) -> None:
        prefix = "test production_daemon_path_runs_chrome_and_reaps_owned_profile ... "
        suite.verify_cases(prefix + passing_output())

    def test_case_inside_other_text_is_not_counted(self) -> None:
        output = passing_output().replace(
            "PARITY_CASE ENG-007.download PASS", "note: PARITY_CASE ENG-007.download PASS"
        )
        with self.assertRaisesRegex(AssertionError, "ENG-007.download"):
            suite.verify_cases(output)

    def test_skipped_run_is_rejected(self) -> None:
        with self.assertRaisesRegex(AssertionError, "missing"):
            suite.verify_cases("test result: ok. 1 passed; 0 failed; 0 ignored\n")

    def test_missing_case_is_rejected(self) -> None:
        output = passing_output().replace("PARITY_CASE ENG-007.download PASS\n", "")
        with self.assertRaisesRegex(AssertionError, "ENG-007.download"):
            suite.verify_cases(output)

    def test_unexpected_and_duplicate_cases_are_rejected(self) -> None:
        with self.assertRaisesRegex(AssertionError, "unexpected"):
            suite.verify_cases(passing_output() + "PARITY_CASE ENG-009.made-up PASS\n")
        with self.assertRaisesRegex(AssertionError, "duplicates"):
            suite.verify_cases(passing_output() + "PARITY_CASE ENG-005.form-input PASS\n")


class ReplayVerificationTests(unittest.TestCase):
    def test_accepts_the_documented_gap_only(self) -> None:
        line = "oracle replay: 120 matched, 1 mismatched (1 known gaps) of 121 steps"
        self.assertEqual(suite.verify_replay(line, 121), (120, 121))

    def test_rejects_skip_and_new_gaps(self) -> None:
        with self.assertRaisesRegex(AssertionError, "did not run"):
            suite.verify_replay("SKIP oracle replay: set SYMBROWSE_LIVE_CHROME=1", 121)
        with self.assertRaisesRegex(AssertionError, "known gaps"):
            suite.verify_replay("oracle replay: 119 matched, 2 mismatched (2 known gaps) of 121 steps", 121)
        with self.assertRaisesRegex(AssertionError, "expected 122"):
            suite.verify_replay("oracle replay: 120 matched, 1 mismatched (1 known gaps) of 121 steps", 122)


if __name__ == "__main__":
    unittest.main()
