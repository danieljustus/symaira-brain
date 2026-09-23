import importlib.util
import json
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


class FixtureRootNormalizationTests(unittest.TestCase):
    def test_normalizes_raw_and_json_escaped_root_prefix(self):
        root = Path(r"C:\tmp\go")
        raw = (str(root) + r"\config\file").encode()
        escaped = json.dumps(str(root) + r"\config\file").encode()
        self.assertEqual(
            DIFFERENTIAL.normalize_fixture_root(raw, root),
            b"<root>\\config\\file",
        )
        self.assertEqual(
            DIFFERENTIAL.normalize_fixture_root(escaped, root),
            b'"<root>\\\\config\\\\file"',
        )

    def test_normalizes_only_random_atomic_rename_source_suffix(self):
        go_error = b"rename C:\\config.toml.tmp-239445890 C:\\config.toml: Access is denied."
        rust_error = b"rename C:\\config.toml.tmp-r4nd0m C:\\config.toml: Access is denied."
        expected = b"rename C:\\config.toml.tmp-<random> C:\\config.toml: Access is denied."
        self.assertEqual(DIFFERENTIAL.normalize_atomic_tempfile(go_error), expected)
        self.assertEqual(DIFFERENTIAL.normalize_atomic_tempfile(rust_error), expected)


if __name__ == "__main__":
    unittest.main()
