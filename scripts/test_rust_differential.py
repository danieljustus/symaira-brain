import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("rust_differential", SCRIPTS / "rust-differential.py")
assert SPEC is not None and SPEC.loader is not None
DIFFERENTIAL = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = DIFFERENTIAL
SPEC.loader.exec_module(DIFFERENTIAL)


class InstallAtimePlatformTests(unittest.TestCase):
    def test_go_install_atime_support_matches_build_tags(self):
        for goos in ("linux", "darwin"):
            with self.subTest(goos=goos):
                self.assertTrue(DIFFERENTIAL.go_reports_install_atime(goos))
        for goos in ("win32", "windows", "freebsd", "openbsd"):
            with self.subTest(goos=goos):
                self.assertFalse(DIFFERENTIAL.go_reports_install_atime(goos))


if __name__ == "__main__":
    unittest.main()
