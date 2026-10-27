import contextlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

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

    def test_binds_owned_roots_inside_quoted_json_errors_without_hiding_error_changes(self):
        first_root = Path(r"C:\owned-fixture-a")
        second_root = Path(r"C:\owned-fixture-b")

        def error(root, token="The system cannot find the path specified."):
            path = str(root) + r"\receiving sources\tools\go"
            quoted = json.dumps(path)
            return json.dumps({"error": f"exec: {quoted}: GetFileAttributesEx {path}: {token}",
                               "unowned": r"D:\unrelated\tools\go"}).encode()

        first = DIFFERENTIAL.normalize_fixture_root(error(first_root), first_root)
        second = DIFFERENTIAL.normalize_fixture_root(error(second_root), second_root)
        self.assertEqual(first, second)
        self.assertIn(b'D:\\\\unrelated\\\\tools\\\\go', first)
        self.assertNotEqual(first, DIFFERENTIAL.normalize_fixture_root(
            error(second_root, "different error cause"), second_root))
        self.assertNotEqual(first, DIFFERENTIAL.normalize_fixture_root(error(second_root), first_root))

    def test_normalizes_only_random_atomic_rename_source_suffix(self):
        go_error = b"rename C:\\config.toml.tmp-239445890 C:\\config.toml: Access is denied."
        rust_error = b"rename C:\\config.toml.tmp-r4nd0m C:\\config.toml: Access is denied."
        expected = b"rename C:\\config.toml.tmp-<random> C:\\config.toml: Access is denied."
        self.assertEqual(DIFFERENTIAL.normalize_atomic_tempfile(go_error), expected)
        self.assertEqual(DIFFERENTIAL.normalize_atomic_tempfile(rust_error), expected)


class _NoServer:
    def __enter__(self):
        return "http://127.0.0.1:0"

    def __exit__(self, *_args):
        return None


class InjectedTimeoutTests(unittest.TestCase):
    """A hung child must not discard comparison failures collected before it."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.bins = Path(tmp.name)
        for name in ("go-bin", "rust-bin"):
            (self.bins / name).write_bytes(name.encode())

    def run_main(self, journal_root=None):
        cases = (
            DIFFERENTIAL.Case("tmo_first_mismatch", ("one",)),
            DIFFERENTIAL.Case("tmo_second_hangs", ("two",)),
            DIFFERENTIAL.Case("tmo_third_matches", ("three",)),
        )
        seen = []

        def fake_run(binary, argv, env, stdin=None, use_pty=False):
            seen.append((Path(binary).name, argv[0]))
            if argv[0] == "two" and Path(binary).name == "rust-bin":
                raise subprocess.TimeoutExpired([str(binary), *argv], 10)
            out = b"rust\n" if argv[0] == "one" and Path(binary).name == "rust-bin" else b"same\n"
            return subprocess.CompletedProcess([str(binary), *argv], 0, out, b"")

        env = {k: v for k, v in os.environ.items() if k != "SYMBRAIN_DIFFERENTIAL_CASE_PREFIX"}
        if journal_root is not None:
            env["SYMBRAIN_DIFFERENTIAL_JOURNAL"] = str(journal_root)
        else:
            env.pop("SYMBRAIN_DIFFERENTIAL_JOURNAL", None)
        stdout, stderr = io.StringIO(), io.StringIO()
        with mock.patch.object(DIFFERENTIAL, "CASES", cases), \
                mock.patch.object(DIFFERENTIAL, "run", fake_run), \
                mock.patch.object(DIFFERENTIAL, "ensure_external_environment", lambda _f: None), \
                mock.patch.object(DIFFERENTIAL, "ReleaseFixtureServer", _NoServer), \
                mock.patch.object(sys, "argv", ["rust-differential.py", str(self.bins / "go-bin"), str(self.bins / "rust-bin")]), \
                mock.patch.dict(os.environ, env, clear=True), \
                contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = DIFFERENTIAL.main()
        return code, stdout.getvalue(), stderr.getvalue(), seen

    def test_timeout_keeps_earlier_failures_and_still_fails(self):
        code, stdout, stderr, seen = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("tmo_first_mismatch: Go=", stderr)
        lines = [line for line in stderr.splitlines() if line.startswith("TIMEOUT case=tmo_second_hangs ")]
        self.assertEqual(len(lines), 1, stderr)
        self.assertIn("timeout=10s", lines[0])
        # The run continues after the hung case and never reports a clean pass.
        self.assertIn("PASS tmo_third_matches", stdout)
        self.assertIn(("rust-bin", "three"), seen)
        self.assertNotIn("parity cases passed", stdout)
        self.assertNotIn("PASS tmo_second_hangs", stdout)

    def test_timeout_is_journaled_per_case_with_distinct_root(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "journal"
            code, _stdout, stderr, _seen = self.run_main(root)
            self.assertEqual(code, 1)
            rows = [json.loads(line) for line in (root / "phases.jsonl").read_text().splitlines()]
            timeouts = [row for row in rows if row["phase"] == "child_timeout"]
            self.assertEqual([row["case"] for row in timeouts], ["tmo_second_hangs"])
            self.assertEqual(timeouts[0]["runtime"], "Rust")
            self.assertIn("TIMEOUT case=tmo_second_hangs ", stderr)
            # A second replay must not reuse the root (no silent overwrite).
            with self.assertRaises(FileExistsError):
                self.run_main(root)

    def test_unexpected_abort_still_emits_collected_failures(self):
        cases = (
            DIFFERENTIAL.Case("abort_first_mismatch", ("one",)),
            DIFFERENTIAL.Case("abort_second_boom", ("two",)),
        )

        def fake_run(binary, argv, env, stdin=None, use_pty=False):
            if argv[0] == "two":
                raise RuntimeError("boom")
            out = b"rust\n" if Path(binary).name == "rust-bin" else b"go\n"
            return subprocess.CompletedProcess([str(binary), *argv], 0, out, b"")

        stderr = io.StringIO()
        with mock.patch.object(DIFFERENTIAL, "CASES", cases), \
                mock.patch.object(DIFFERENTIAL, "run", fake_run), \
                mock.patch.object(DIFFERENTIAL, "ensure_external_environment", lambda _f: None), \
                mock.patch.object(DIFFERENTIAL, "ReleaseFixtureServer", _NoServer), \
                mock.patch.object(sys, "argv", ["rust-differential.py", str(self.bins / "go-bin"), str(self.bins / "rust-bin")]), \
                mock.patch.dict(os.environ, {k: v for k, v in os.environ.items()
                                             if not k.startswith("SYMBRAIN_DIFFERENTIAL")}, clear=True), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
            with self.assertRaises(RuntimeError):
                DIFFERENTIAL.main()
        self.assertIn("abort_first_mismatch: Go=", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
