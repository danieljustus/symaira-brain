import unittest
from types import SimpleNamespace
from unittest.mock import patch

import run


class EndpointTest(unittest.TestCase):
    def test_windows_go_and_rust_use_the_same_pipe(self):
        with patch.object(run, "os", SimpleNamespace(name="nt")):
            self.assertEqual(run.daemon_endpoint("bench", {}), r"\\.\pipe\symbrowse-bench")


if __name__ == "__main__":
    unittest.main()
