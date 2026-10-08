#!/usr/bin/env python3
"""Checkpoint transport regression, not a native process-parity fixture."""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from run import checkpoint


class CheckpointTest(unittest.TestCase):
    def test_failure_and_killed_writer_preserve_last_complete_observation(self):
        with tempfile.TemporaryDirectory(prefix="skills-checkpoint-") as directory:
            output = Path(directory) / "capture.json"
            previous = {"status": "running", "native_surface_gate_passed": False,
                        "results": [{"id": "owned", "matched": False, "raw": "a\u2028b\r\n"}]}
            checkpoint(output, previous)
            original = output.read_bytes()
            self.assertEqual(json.loads(original), previous)
            updated = {**previous, "status": "failed", "error": {"type": "ObservedMismatch"}}
            with patch("run.os.replace", side_effect=OSError("owned publication failure")):
                with self.assertRaisesRegex(OSError, "owned publication failure"):
                    checkpoint(output, updated)
            self.assertEqual(output.read_bytes(), original)
            self.assertEqual(list(output.parent.iterdir()), [output])

            # Real child reaches publication before it is killed; no timing guess.
            source = (
                "import sys, threading\n"
                "from pathlib import Path\n"
                "import run\n"
                "def stopped_publication(*args):\n"
                "    print('staged', flush=True)\n"
                "    threading.Event().wait(30)\n"
                "    raise RuntimeError('parent did not terminate owned writer')\n"
                "run.os.replace = stopped_publication\n"
                "run.checkpoint(Path(sys.argv[1]), {'status':'running','results':[]})\n"
            )
            child = subprocess.Popen([sys.executable, "-B", "-c", source, str(output)],
                                     cwd=Path(__file__).resolve().parent,
                                     env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
                                     stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE)
            stdout, stderr_stream = child.stdout, child.stderr
            assert stdout is not None and stderr_stream is not None
            ready = queue.Queue()
            reader = threading.Thread(target=lambda: ready.put(stdout.readline()), daemon=True)
            reader.start()
            try:
                self.assertEqual(ready.get(timeout=10), b"staged\n")
                child.kill()
                _, stderr = child.communicate(timeout=10)
                self.assertFalse(stderr)
                self.assertNotEqual(child.returncode, 0)
            finally:
                if child.poll() is None:
                    child.kill()
                child.wait(timeout=10)
                reader.join(timeout=10)
                stdout.close()
                stderr_stream.close()
            self.assertEqual(output.read_bytes(), original)
            self.assertEqual(json.loads(output.read_bytes()), previous)
            checkpoint(output, updated)
            self.assertEqual(json.loads(output.read_bytes()), updated)


if __name__ == "__main__":
    unittest.main()
