"""Portable owned-file checks; no Go, Cargo, product process or Windows claim."""
import base64
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from progress import Journal, diagnostic_tree, raw


class ProgressTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="source-journal-owned-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.journal = Journal(self.root / "report.json", [{"name":"owned-case"}],
                               None, {"binary_sha256":"owned-synthetic-identity"})

    def published(self):
        return json.loads(self.journal.path.read_bytes())

    def test_initial_real_write_and_atomic_replacement(self):
        original = self.journal.path.read_bytes()
        replace = __import__("os").replace
        observed = []

        def replacement(source, destination):
            # Until replacement, readers see the complete prior document.
            self.assertEqual(self.journal.path.read_bytes(), original)
            candidate = json.loads(Path(source).read_bytes())
            self.assertEqual(candidate["events"][0]["phase"], "owned-launch")
            observed.append((Path(source).parent, Path(destination)))
            replace(source, destination)

        with patch("progress.os.replace", side_effect=replacement):
            self.journal.event("owned-launch", argv=["owned", "argument with spaces"])
        self.assertEqual(observed, [(self.root, self.journal.path)])
        self.assertEqual(self.published()["events"][0]["argv"], ["owned", "argument with spaces"])
        self.assertEqual(list(self.root.iterdir()), [self.journal.path])

    def test_replace_failure_keeps_prior_document_and_removes_temporary(self):
        original = self.journal.path.read_bytes()
        with patch("progress.os.replace", side_effect=OSError(5, "owned injected replacement failure")):
            with self.assertRaisesRegex(OSError, "owned injected replacement failure"):
                self.journal.event("owned-failed-write")
        self.assertEqual(self.journal.path.read_bytes(), original)
        self.assertEqual(list(self.root.iterdir()), [self.journal.path])

    def test_binary_streams_and_missing_streams_remain_distinct(self):
        value = bytes(range(256)) + b"\r\n\x00\xff\xe2\x82"
        self.journal.event("owned-return", stdout_base64=raw(value),
                           stderr_base64=raw(b""), timeout_stdout_base64=raw(None))
        event = self.published()["events"][0]
        self.assertEqual(base64.b64decode(event["stdout_base64"]), value)
        self.assertEqual(event["stderr_base64"], "")
        self.assertIsNone(event["timeout_stdout_base64"])

    def test_failure_retains_completed_pair_and_actual_exception(self):
        pair = {"case":"owned-case", "go":{"actual_exit":0}, "rust":{"actual_exit":0}}
        self.journal.pair(pair)
        self.journal.failed(TimeoutError("owned child timeout"))
        data = self.published()
        self.assertEqual(data["status"], "failed")
        self.assertEqual(data["exception_type"], "TimeoutError")
        self.assertEqual(data["exception"], "owned child timeout")
        self.assertEqual(data["complete_pairs"], [pair])

    def test_finish_retains_exact_comparison_exit_without_promoting_failure(self):
        self.journal.event("owned-comparison-returned", exit=1)
        self.journal.finish({"exit":1, "complete_observations":1})
        data = self.published()
        self.assertEqual(data["status"], "complete")
        self.assertEqual(data["comparison_exit"], 1)
        self.assertEqual(data["complete_observations"], 1)
        self.assertEqual(data["events"][0]["exit"], 1)

    def test_owned_failure_capture_preserves_full_binary_and_hash(self):
        fixture = self.root / "fixture"
        fixture.mkdir()
        value = bytes(range(256)) + b"owned tool bytes\x00\xff\xe2\x82\r\n"
        capture = fixture / ".symbrain-source-capture-owned"
        capture.write_bytes(value)
        observation = diagnostic_tree(fixture)
        self.assertTrue(observation["inventory_complete"])
        self.assertFalse(observation["errors"])
        row = next(row for row in observation["rows"] if row["path"] == capture.name)
        self.assertTrue(row["raw_bytes_complete"])
        self.assertTrue(row["sha256_complete"])
        self.assertEqual(base64.b64decode(row["raw_bytes_base64"]), value)
        self.assertEqual(row["sha256"], hashlib.sha256(value).hexdigest())
        self.journal.event("owned-failure-snapshot", fixture=observation)
        retained = self.published()["events"][0]["fixture"]
        self.assertEqual(retained, observation)


if __name__ == "__main__":
    unittest.main(verbosity=2)
