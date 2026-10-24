"""Classification and evidence tests for the skills-status mutant runner.

Run with: python3 -m unittest discover -s scripts/skills-status-config-oracle -p test_run.py -v
"""
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("skills_status_mutants", HERE / "run.py")
assert SPEC is not None and SPEC.loader is not None
RUN = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RUN
SPEC.loader.exec_module(RUN)

EXPECT = ["case_a", "case_b"]


class ClassifyTests(unittest.TestCase):
    def test_killed_requires_nonzero_exit_and_every_expected_case(self):
        stderr = "case_a: Go=1, Rust=2\ncase_b: Go=1, Rust=2\n"
        self.assertEqual(RUN.classify(1, stderr, EXPECT, [])[0], RUN.KILLED)

    def test_missing_expected_case_or_clean_exit_survives(self):
        self.assertEqual(RUN.classify(1, "case_a: Go=1, Rust=2\n", EXPECT, [])[0], RUN.SURVIVED)
        self.assertEqual(RUN.classify(0, "", EXPECT, [])[0], RUN.SURVIVED)

    def test_expected_name_inside_other_text_is_not_a_failure(self):
        stderr = "other: Go=case_a: x, Rust=case_b: y\n"
        self.assertEqual(RUN.failed_cases(EXPECT, stderr), [])

    def test_timeout_is_inconclusive_even_when_expected_cases_failed(self):
        stderr = ("case_a: Go=1, Rust=2\ncase_b: Go=1, Rust=2\n"
                  "TIMEOUT case=case_z timeout=10s binary='x' argv='y'; comparison for this case was not completed\n")
        timeouts = RUN.timed_out_cases(stderr, None)
        self.assertEqual(timeouts, ["case_z"])
        self.assertEqual(RUN.classify(1, stderr, EXPECT, timeouts)[:2], (RUN.INCONCLUSIVE, "timeout"))

    def test_timeout_case_is_never_counted_as_expected_failure(self):
        stderr = "TIMEOUT case=case_a timeout=10s binary='x' argv='y'; comparison for this case was not completed\n"
        self.assertEqual(RUN.failed_cases(EXPECT, stderr), [])
        status, reason, _ = RUN.classify(1, stderr, EXPECT, RUN.timed_out_cases(stderr, None))
        self.assertEqual((status, reason), (RUN.INCONCLUSIVE, "timeout"))

    def test_journal_timeout_event_is_found_without_stderr_line(self):
        with tempfile.TemporaryDirectory() as tmp:
            journal = Path(tmp)
            rows = [{"phase": "child_begin", "case": "case_a"},
                    {"phase": "child_timeout", "case": "case_b", "runtime": "Rust"}]
            (journal / "phases.jsonl").write_text("\n".join(json.dumps(r) for r in rows) + "\n")
            self.assertEqual(RUN.timed_out_cases("", journal), ["case_b"])

    def test_harness_failures_are_inconclusive(self):
        self.assertEqual(RUN.classify(2, "usage", EXPECT, [])[:2], (RUN.INCONCLUSIVE, "harness"))
        self.assertEqual(RUN.classify(1, "Traceback (most recent call last):\n", EXPECT, [])[:2],
                         (RUN.INCONCLUSIVE, "harness"))


class ReplayEvidenceTests(unittest.TestCase):
    def test_build_failure_is_inconclusive_build_and_keeps_logs(self):
        import subprocess
        from unittest import mock

        error = subprocess.CalledProcessError(101, ["cargo"], output="out-text", stderr="err-text")
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(RUN, "build_mutant", side_effect=error):
            evidence = Path(tmp)
            record = RUN.replay(1, "demo", ("a", "b", EXPECT), EXPECT, Path("/x/go"), Path(tmp) / "t", evidence)
            self.assertEqual((record["status"], record["reason"]), (RUN.INCONCLUSIVE, "build"))
            folder = evidence / "01-demo"
            self.assertEqual((folder / "build.stderr").read_text(), "err-text")
            self.assertEqual(json.loads((folder / "result.json").read_text())["reason"], "build")

    def test_timed_out_replay_keeps_streams_digest_executable_and_unique_journal(self):
        import hashlib
        import subprocess
        from unittest import mock

        def fake_build(name, target, old, new, dest):
            path = dest / f"symbrain-{name}"
            path.write_bytes(b"mutant-bytes")
            return path

        def fake_differential(go, binary, journal=None):
            journal.mkdir(parents=True)
            (journal / "phases.jsonl").write_text(
                json.dumps({"phase": "child_timeout", "case": "case_slow", "runtime": "Rust"}) + "\n")
            return subprocess.CompletedProcess([], 1, "partial-out\n", "TIMEOUT case=case_slow timeout=10s x\n")

        with tempfile.TemporaryDirectory() as tmp, \
                mock.patch.object(RUN, "build_mutant", fake_build), \
                mock.patch.object(RUN, "differential", fake_differential):
            evidence = Path(tmp)
            record = RUN.replay(3, "demo", ("a", "b", EXPECT), EXPECT, Path("/x/go"), Path(tmp) / "t", evidence)
            folder = evidence / "03-demo"
            self.assertEqual((record["status"], record["reason"]), (RUN.INCONCLUSIVE, "timeout"))
            self.assertEqual(record["timed_out_cases"], ["case_slow"])
            self.assertEqual(record["mutant_sha256"], hashlib.sha256(b"mutant-bytes").hexdigest())
            self.assertEqual((folder / "stdout.txt").read_text(), "partial-out\n")
            self.assertIn("case_slow", (folder / "stderr.txt").read_text())
            self.assertEqual((folder / "symbrain-demo").read_bytes(), b"mutant-bytes")
            # The same label cannot be replayed into the same evidence folder.
            with self.assertRaises(FileExistsError):
                RUN.replay(3, "demo", ("a", "b", EXPECT), EXPECT, Path("/x/go"), Path(tmp) / "t", evidence)


if __name__ == "__main__":
    unittest.main()
