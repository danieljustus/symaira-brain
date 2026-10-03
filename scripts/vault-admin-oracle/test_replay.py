"""Guard exact diagnostic comparisons across Windows temporary roots."""
import importlib.util
import json
from pathlib import Path, PureWindowsPath
import unittest

spec = importlib.util.spec_from_file_location("vault_replay", Path(__file__).with_name("replay.py"))
replay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(replay)


class RootNormalization(unittest.TestCase):
    def test_raw_and_go_quoted_windows_roots_normalize_without_hiding_errors(self):
        go = PureWindowsPath(r"C:\Users\runner\Temp\go-53")
        rust = PureWindowsPath(r"C:\Users\runner\Temp\rust-53")

        def message(root, suffix):
            return ("configured " + json.dumps(str(root / "missing"))
                    + ": stat " + str(root / "missing") + ": " + suffix).encode()

        expected = replay.normalize_output(message(go, "not found"), go)
        actual = replay.normalize_output(message(rust, "not found"), rust)
        self.assertEqual(expected, actual)
        self.assertIn('"<ROOT>\\\\missing"', actual)
        self.assertIn("stat <ROOT>\\missing", actual)
        self.assertNotEqual(expected, replay.normalize_output(message(rust, "permission denied"), rust))
        # A path outside this process's disposable root is still significant.
        outside = message(PureWindowsPath(r"C:\other\rust-53"), "not found")
        self.assertEqual(replay.normalize_output(outside, rust), outside.decode())
