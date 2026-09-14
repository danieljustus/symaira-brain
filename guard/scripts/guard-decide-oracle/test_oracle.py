import copy
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import time
import unittest

HERE = Path(__file__).parent
spec = importlib.util.spec_from_file_location("oracle", HERE / "oracle.py")
assert spec is not None and spec.loader is not None
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)


class OracleHarnessTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest_path = oracle.EVIDENCE / "manifest.json"
        if not cls.manifest_path.is_file():
            raise unittest.SkipTest("run oracle.py before mutation tests")
        cls.original = json.loads(cls.manifest_path.read_text())
        cls.expected_digest = oracle.digest(cls.manifest_path.read_bytes())

    def assert_manifest_rejected(self, mutate):
        altered = copy.deepcopy(self.original)
        mutate(altered)
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
            json.dump(altered, f, sort_keys=True)
            path = Path(f.name)
        try:
            with self.assertRaises(AssertionError):
                oracle.validate_manifest(path)
        finally:
            path.unlink()

    def test_validator_rejects_output_hash_mutation(self):
        self.assert_manifest_rejected(lambda m: m["results"][0]["stdout"].__setitem__("sha256", "0" * 64))

    def test_validator_rejects_cases_hash_mutation(self):
        self.assert_manifest_rejected(lambda m: m["cases"].__setitem__("sha256", "0" * 64))

    def test_validator_rejects_source_hash_mutation(self):
        self.assert_manifest_rejected(lambda m: m["source_identity"]["tracked_files"][0].__setitem__("sha256", "0" * 64))

    def test_validator_rejects_binary_hash_mutation(self):
        self.assert_manifest_rejected(lambda m: m["binary"].__setitem__("sha256", "0" * 64))

    def test_validator_rejects_wrong_supplied_digest(self):
        with self.assertRaises(AssertionError):
            oracle.validate_manifest(self.manifest_path, expected_digest="0" * 64)

    def test_validator_rejects_absolute_evidence_path(self):
        self.assert_manifest_rejected(lambda m: m["results"][0]["stdout"].__setitem__("path", str(Path(tempfile.gettempdir()) / "sentinel")))

    def test_validator_rejects_traversal_evidence_path(self):
        self.assert_manifest_rejected(lambda m: m["results"][0]["stdout"].__setitem__("path", "cases/../outside"))

    def test_validator_rejects_symlink_escape(self):
        with tempfile.TemporaryDirectory() as td:
            outside = Path(td) / "outside"; outside.write_bytes(b"harmless sentinel")
            link = oracle.EVIDENCE / "test-escape-link"
            try:
                link.symlink_to(outside)
                altered = copy.deepcopy(self.original)
                altered["results"][0]["stdout"]["path"] = "test-escape-link"
                with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
                    json.dump(altered, f, sort_keys=True); path = Path(f.name)
                try:
                    with self.assertRaises(AssertionError): oracle.validate_manifest(path)
                finally: path.unlink()
            finally:
                link.unlink(missing_ok=True)

    def test_validator_rejects_declared_byte_count_mutation(self):
        self.assert_manifest_rejected(lambda m: m["results"][0]["stdout"].__setitem__("bytes", m["results"][0]["stdout"]["bytes"] + 1))

    def test_validator_rejects_malformed_commit(self):
        self.assert_manifest_rejected(lambda m: m["source_identity"].__setitem__("commit_sha", "not-a-sha"))

    def test_validator_rejects_wrong_commit(self):
        self.assert_manifest_rejected(lambda m: m["source_identity"].__setitem__("commit_sha", "1" * 40))

    def test_validator_rejects_toolchain_mutation(self):
        self.assert_manifest_rejected(lambda m: m["toolchain"].__setitem__("selected_executable_sha256", "0" * 64))

    def test_normal_completion_is_not_timeout(self):
        result = oracle.run_bounded(["python3", "-c", "print('ok')"], cwd=Path.cwd(), env={"PATH": "/usr/bin:/bin"}, stdin=None, timeout=2.0)
        self.assertFalse(result.timed_out)
        self.assertEqual(result.returncode, 0)

    def test_bounded_group_cleanup(self):
        with tempfile.TemporaryDirectory() as td:
            marker = Path(td) / "descendant"
            script = ("import os,time; p=os.fork();\n"
                      "if p==0: open(%r,'w').close(); time.sleep(0.8); open(%r,'w').close()\n"
                      "else: time.sleep(30)") % (str(marker) + ".started", str(marker) + ".survived")
            started = time.monotonic()
            result = oracle.run_bounded(["python3", "-c", script], cwd=Path(td), env={"PATH": "/usr/bin:/bin"}, stdin=None, timeout=0.3)
            self.assertLess(time.monotonic() - started, 4)
            self.assertTrue(result.timed_out)
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(Path(str(marker) + ".started").exists())
            time.sleep(1.0)
            self.assertFalse(Path(str(marker) + ".survived").exists())


if __name__ == "__main__":
    unittest.main()
