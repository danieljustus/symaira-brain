import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class ResourceProbeTests(unittest.TestCase):
    def test_measures_positive_os_peak_for_one_cli_process(self):
        helper = Path(__file__).with_name("resource_probe.py")
        with tempfile.TemporaryDirectory(prefix="bench-rss-") as root:
            request = {
                "command": [sys.executable, "-c", "print('probe-ok')"],
                "cwd": root,
                "env": os.environ.copy(),
                "timeout": 5,
            }
            result = subprocess.run(
                [sys.executable, str(helper)],
                input=json.dumps(request),
                text=True,
                capture_output=True,
                timeout=10,
                check=True,
            )
        measured = json.loads(result.stdout)
        self.assertEqual(measured["returncode"], 0)
        self.assertEqual(measured["stdout"], "probe-ok\n")
        self.assertGreater(measured["duration_ns"], 0)
        self.assertIsInstance(measured["peak_rss_bytes"], int)
        self.assertGreater(measured["peak_rss_bytes"], 0)
        self.assertTrue(measured["peak_rss_method"])


if __name__ == "__main__":
    unittest.main()
