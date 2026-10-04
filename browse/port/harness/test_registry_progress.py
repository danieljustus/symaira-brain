#!/usr/bin/env python3
"""Real subprocess progress survives before completion and preserves raw witnesses."""
import base64
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest

import daemon_registry_cli as edges
from registry_progress import Progress, MAX_EVENTS, MAX_EVENT_BYTES

ENV = {name: os.environ[name] for name in ("SystemRoot", "windir", "ComSpec", "PATHEXT") if name in os.environ}


class ProgressTests(unittest.TestCase):
    def test_real_child_begin_is_readable_while_child_is_running(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); journal = Progress(root / "progress.jsonl", {"source": "owned-fixture"})
            results = []
            def execute():
                results.append(edges.execute(Path(sys.executable), root, ENV,
                    [b"-c", b"import time; time.sleep(.3)"], journal))
            worker = threading.Thread(target=execute); worker.start()
            try:
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline:
                    lines = [json.loads(line) for line in journal.path.read_text().splitlines(keepends=True) if line.endswith("\n")]
                    if any(line["stage"] == "cli.begin" for line in lines): break
                    time.sleep(.002)
                self.assertTrue(any(line["stage"] == "cli.begin" for line in lines))
                self.assertFalse(any(line["stage"] == "cli.end" for line in lines))
                self.assertTrue(worker.is_alive())
            finally:
                worker.join(timeout=3)
            self.assertFalse(worker.is_alive()); self.assertEqual(results[0]["exit"], 0)
            self.assertEqual(json.loads(journal.path.read_text().splitlines()[-1])["stage"], "cli.end")

    def test_real_nonutf8_output_and_failure_are_hashed_without_projection(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); journal = Progress(root / "progress.jsonl", {})
            observed = edges.execute(Path(sys.executable), root, ENV,
                [b"-c", b"import os; os.write(1,b'\\xff'); os.write(2,b'\\x00'); raise SystemExit(7)"], journal)
            complete = json.loads(journal.path.read_text().splitlines()[-1])
            self.assertEqual(observed["exit"], 7); self.assertEqual(complete["exit"], 7)
            self.assertEqual(base64.b64decode(observed["stdout_base64"]), b"\xff")
            self.assertEqual(complete["stdout_sha256"], hashlib.sha256(b"\xff").hexdigest())
            self.assertEqual(complete["stderr_sha256"], hashlib.sha256(b"\0").hexdigest())

    def test_concurrent_cases_keep_complete_lines_and_distinct_case_links(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); journal = Progress(root / "progress.jsonl", {})
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                results = list(pool.map(lambda _: edges.execute(Path(sys.executable), root, ENV,
                    [b"-c", b"print('owned')"], journal), range(8)))
            self.assertTrue(all(result["exit"] == 0 for result in results))
            lines = [json.loads(line) for line in journal.path.read_text().splitlines()]
            starts = {line["sequence"] for line in lines if line["stage"] == "cli.begin"}
            ends = {line["case"] for line in lines if line["stage"] == "cli.end"}
            self.assertEqual(len(starts), 8); self.assertEqual(starts, ends)
            self.assertEqual([line["sequence"] for line in lines], list(range(1, 18)))

    def test_raw_argv_identity_survives_receipt(self):
        with tempfile.TemporaryDirectory() as folder:
            journal = Progress(Path(folder) / "progress.jsonl", {})
            arguments = [b"bad\xffsession", b"bad\xc0\xafsession"]
            journal.begin_cli(Path(sys.executable), arguments)
            record = json.loads(journal.path.read_text().splitlines()[-1])
            self.assertEqual([base64.b64decode(value) for value in record["arguments_filesystem_base64"]], arguments)

    def test_bounds_do_not_replace_already_written_evidence(self):
        with tempfile.TemporaryDirectory() as folder:
            journal = Progress(Path(folder) / "progress.jsonl", {})
            original = journal.path.read_bytes()
            with self.assertRaisesRegex(RuntimeError, "record bound"):
                journal.event("oversized", value="x" * MAX_EVENT_BYTES)
            self.assertEqual(journal.path.read_bytes(), original)
            journal.sequence = MAX_EVENTS
            with self.assertRaisesRegex(RuntimeError, "event bound"):
                journal.event("overflow")
            self.assertEqual(journal.path.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
