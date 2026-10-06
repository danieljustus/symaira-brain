"""Portable owned Python children verify captures, not daemon/native parity."""
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import registry_cli_process as capture
from registry_progress import Progress

ENV = {name: os.environ[name] for name in ("SystemRoot", "windir", "ComSpec") if name in os.environ}


class CaptureTests(unittest.TestCase):
    def setup_owned(self, folder):
        root = Path(folder)
        return root, Progress(root / "progress.jsonl", {"scope": "owned Python capture fixture"})

    def receipt(self, progress):
        events = [json.loads(line) for line in progress.path.read_text().splitlines()]
        end = events[-1]
        self.assertEqual(end["stage"], "cli.end")
        return events, json.loads(Path(end["raw_receipt"]).read_text())

    def test_nonzero_and_non_utf8_streams_are_preserved_before_caller_validation(self):
        with tempfile.TemporaryDirectory() as folder:
            root, journal = self.setup_owned(folder)
            result = capture.capture(Path(sys.executable), root, ENV,
                ["-c", "import os; os.write(1,b'\\xff'); os.write(2,b'\\x00err'); raise SystemExit(7)"], journal)
            events, record = self.receipt(journal)
            self.assertEqual(result.returncode, 7)
            self.assertEqual(result.stdout, b"\xff")
            self.assertEqual(result.stderr, b"\x00err")
            self.assertEqual([base64.b64decode(row["base64"]) for row in record["streams"]], [result.stdout, result.stderr])
            self.assertEqual([row["stage"] for row in events], ["binding", "cli.begin", "cli.launch.begin", "cli.spawned", "cli.end"])
            self.assertIsInstance(record["pid"], int)
            self.assertEqual(record["timeout_seconds"], 15)

    def test_owned_writer_holding_sibling_does_not_delay_completed_cli(self):
        with tempfile.TemporaryDirectory() as folder:
            root, journal = self.setup_owned(folder)
            real_spawn, holders = subprocess.Popen, []
            def spawn(*arguments, **kwargs):
                self.assertTrue(hasattr(kwargs["stdout"], "fileno"), "capture must be file-backed")
                holders.append(real_spawn([sys.executable, "-c", "import time; time.sleep(30)"],
                    stdin=subprocess.DEVNULL, stdout=kwargs["stdout"], stderr=kwargs["stderr"], env=ENV))
                return real_spawn(*arguments, **kwargs)
            try:
                with patch.object(capture.subprocess, "Popen", spawn):
                    result = capture.capture(Path(sys.executable), root, ENV, ["-c", "print('completed')"], journal)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, b"completed\n" if os.name == "posix" else b"completed\r\n")
                self.assertIsNone(holders[0].poll(), "holder must remain live at bounded CLI completion")
                _, record = self.receipt(journal)
                self.assertFalse(record["timed_out"])
            finally:
                for child in holders:
                    child.kill()
                    child.wait(timeout=2)

    def test_timeout_is_failure_with_raw_file_pid_and_reaped_status(self):
        with tempfile.TemporaryDirectory() as folder:
            root, journal = self.setup_owned(folder)
            # Only this owned fixture gets a short test deadline; production
            # CLI_TIMEOUT remains15 and the native gate keeps all assertions.
            with patch.object(capture, "CLI_TIMEOUT", .2):
                with self.assertRaises(subprocess.TimeoutExpired):
                    capture.capture(Path(sys.executable), root, ENV,
                        ["-c", "import os,time; os.write(2,b'live'); time.sleep(30)"], journal)
            _, record = self.receipt(journal)
            self.assertTrue(record["timed_out"])
            self.assertIsInstance(record["pid"], int)
            self.assertIsNotNone(record["exit"])
            self.assertIsNone(record["cleanup_error"])

    def test_job_adopts_child_suspended_and_adoption_failure_reaps_it(self):
        with tempfile.TemporaryDirectory() as folder:
            root, journal = self.setup_owned(folder)
            real_spawn, spawned = subprocess.Popen, []
            def spawn(*arguments, creationflags=0, **kwargs):
                spawned.append(creationflags)
                return real_spawn(*arguments, **kwargs)  # Portable stand-in.
            class Job:
                SUSPENDED = 0x4
                def __init__(self, error=None):
                    self.adopted, self.error = [], error
                def adopt(self, child):
                    self.adopted.append(child.pid)
                    if self.error: raise self.error
            job = Job()
            with patch.object(capture.subprocess, "Popen", spawn):
                result = capture.capture(Path(sys.executable), root, ENV, ["-c", "pass"], journal, job)
            self.assertEqual((result.returncode, spawned), (0, [0x4]))
            _, record = self.receipt(journal)
            self.assertEqual(job.adopted, [record["pid"]])
            failing = Job(OSError("assign failed"))
            with patch.object(capture.subprocess, "Popen", spawn):
                with self.assertRaisesRegex(OSError, "assign failed"):
                    capture.capture(Path(sys.executable), root, ENV,
                                    ["-c", "import time; time.sleep(30)"], journal, failing)
            _, record = self.receipt(journal)
            self.assertIsNotNone(record["exit"], "unadopted child must be killed and reaped")
            self.assertIsNone(record["cleanup_error"])

    def test_launch_failure_retains_empty_captures_then_raises(self):
        with tempfile.TemporaryDirectory() as folder:
            root, journal = self.setup_owned(folder)
            with self.assertRaises(OSError):
                capture.capture(root / "nonexistent-owned-executable", root, ENV, [], journal)
            _, record = self.receipt(journal)
            self.assertIsNone(record["pid"])
            self.assertIsNone(record["exit"])
            self.assertTrue(record["failure"])
            self.assertEqual([row["bytes"] for row in record["streams"]], [0, 0])


if __name__ == "__main__":
    unittest.main()
