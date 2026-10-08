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
from registry_cli_process import CLI_TIMEOUT

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
            raw_lines = journal.path.read_bytes().splitlines(keepends=True)
            self.assertTrue(all(line.endswith(b"\n") for line in raw_lines))
            lines = [json.loads(line) for line in raw_lines]
            # One binding plus four events per actual capture. Launch/spawn
            # witnesses are required, not duplicate begin/end observations.
            self.assertEqual([line["sequence"] for line in lines], list(range(1, 34)))
            self.assertEqual(lines[0]["stage"], "binding")
            self.assertEqual([line["monotonic"] for line in lines],
                             sorted(line["monotonic"] for line in lines))
            starts = {line["sequence"]: line for line in lines if line["stage"] == "cli.begin"}
            stages = ("cli.begin", "cli.launch.begin", "cli.spawned", "cli.end")
            self.assertEqual(len(starts), 8)
            self.assertEqual({line["stage"] for line in lines[1:]}, set(stages))
            for stage in stages:
                self.assertEqual(sum(line["stage"] == stage for line in lines), 8)
            for line in lines[1:]:
                if line["stage"] != "cli.begin":
                    self.assertIn(line["case"], starts)
            stdout = b"owned\n" if os.name == "posix" else b"owned\r\n"
            expected_args = [b"-c", b"print('owned')"]
            for case, begin in starts.items():
                linked = [line for line in lines[1:] if line is begin or line.get("case") == case]
                self.assertEqual([line["stage"] for line in linked], list(stages))
                launch, spawned, end = linked[1:]
                self.assertEqual(begin["binary"], str(Path(sys.executable)))
                self.assertEqual([base64.b64decode(value) for value in
                                  begin["arguments_filesystem_base64"]], expected_args)
                folder = journal.path.with_suffix(".cli") / str(case)
                for event in (launch, spawned):
                    self.assertEqual(event["cwd"], str(root))
                    self.assertEqual(event["timeout_seconds"], CLI_TIMEOUT)
                    self.assertEqual(event["stdout_file"], str(folder / "stdout.bin"))
                    self.assertEqual(event["stderr_file"], str(folder / "stderr.bin"))
                self.assertIsInstance(spawned["pid"], int)
                self.assertGreater(spawned["pid"], 0)
                self.assertEqual(end["pid"], spawned["pid"])
                self.assertEqual(end["exit"], 0)
                self.assertFalse(end["timed_out"])
                self.assertIsNone(end["failure"])
                self.assertIsNone(end["cleanup_error"])
                receipt = folder / "result.json"
                self.assertEqual(end["raw_receipt"], str(receipt))
                self.assertEqual(end["raw_receipt_sha256"], hashlib.sha256(receipt.read_bytes()).hexdigest())
                record = json.loads(receipt.read_bytes())
                self.assertEqual(record["case"], case)
                self.assertEqual(record["pid"], spawned["pid"])
                self.assertEqual(record["exit"], 0)
                self.assertFalse(record["timed_out"])
                self.assertIsNone(record["failure"])
                self.assertIsNone(record["cleanup_error"])
                self.assertEqual(record["cwd"], str(root))
                self.assertEqual(record["timeout_seconds"], CLI_TIMEOUT)
                self.assertEqual(record["stream_scope"],
                                 "snapshot at owned CLI wait/cleanup; no descendant wire or exit claim")
                self.assertEqual(base64.b64decode(record["cwd_filesystem_base64"]), os.fsencode(root))
                self.assertLessEqual(record["begin_monotonic"], record["end_monotonic"])
                self.assertEqual([base64.b64decode(value) for value in
                                  record["arguments_filesystem_base64"]],
                                 [os.fsencode(sys.executable), *expected_args])
                self.assertEqual(len(record["streams"]), 2)
                for name, value, stream in zip(("stdout", "stderr"), (stdout, b""), record["streams"]):
                    self.assertEqual(stream["path"], str(folder / (name + ".bin")))
                    self.assertEqual(Path(stream["path"]).read_bytes(), value)
                    self.assertEqual(base64.b64decode(stream["base64"]), value)
                    self.assertEqual(stream["bytes"], len(value))
                    self.assertEqual(stream["sha256"], hashlib.sha256(value).hexdigest())
                    self.assertEqual(end[name + "_bytes"], len(value))
                    self.assertEqual(end[name + "_sha256"], hashlib.sha256(value).hexdigest())

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
