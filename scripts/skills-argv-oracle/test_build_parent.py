"""Real immutable Git-source controls; mocked fetches are not native build evidence."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

import build_parent as BUILDER


class ParentCachePreparation(unittest.TestCase):
    def setUp(self):
        os.umask(0o022)
        temporary = tempfile.TemporaryDirectory(prefix="skills-parent-cache-866-")
        self.addCleanup(temporary.cleanup)
        owned = Path(temporary.name)
        self.source = owned / "source"
        home = owned / "home"
        home.mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(HOME=str(home), USERPROFILE=str(home), GIT_CONFIG_GLOBAL=os.devnull,
                        GIT_CONFIG_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0", CARGO_NET_OFFLINE="true")
        self.git = shutil.which("git")
        if self.git is None:
            raise RuntimeError("installed Git required for immutable parent source controls")
        self.report = {"commands": [], "source_ref": BUILDER.WINDOWS_PARENT}
        for argv, cwd in [
                (["clone", "--shared", "--no-checkout", "--", BUILDER.ROOT, self.source], None),
                (["config", "core.autocrlf", "false"], self.source),
                (["checkout", "--detach", BUILDER.WINDOWS_PARENT], self.source)]:
            result = subprocess.run([self.git, *map(str, argv)], cwd=cwd, env=self.env,
                                    capture_output=True, check=False, timeout=120)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.report["source_before"] = BUILDER.source_files(
            self.git, self.source, BUILDER.WINDOWS_PARENT, self.env, self.report)

    def prepare(self, download):
        actual_run = BUILDER.run

        def command(argv, env, report, cwd=None, timeout=120):
            if argv[0] == "unexecuted-cargo-control":
                self.assertEqual(argv[1:], ["fetch", "--locked"])
                self.assertEqual(cwd, self.source)
                self.assertEqual(timeout, 300)
                self.assertEqual(env["CARGO_NET_OFFLINE"], "false")
                return download()
            return actual_run(argv, env, report, cwd, timeout)

        try:
            with mock.patch.object(BUILDER, "run", side_effect=command):
                BUILDER.prepare_registry_cache("unexecuted-cargo-control", self.git,
                                               self.source, self.env, self.report)
        finally:
            self.assertEqual(self.env["CARGO_NET_OFFLINE"], "true")

    def test_preparation_preserves_locked_source_and_offline_build_environment(self):
        self.prepare(lambda: b"")
        self.assertEqual(self.report["registry_cache_preparation"],
                         {"status": "passed", "build_network": False})
        self.assertEqual(self.report["source_before"], self.report["source_after_cache_preparation"])

    def test_changed_parent_lockfile_is_rejected(self):
        def download():
            lock = self.source / "Cargo.lock"
            lock.write_bytes(lock.read_bytes() + b"\n# actual changed lockfile control\n")
            return b""
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            self.prepare(download)
        self.assertEqual(self.report["registry_cache_preparation"]["status"], "started")

    def test_untracked_parent_source_is_rejected(self):
        def download():
            (self.source / "actual-untracked-control").write_bytes(b"changed fixture")
            return b""
        with self.assertRaisesRegex(ValueError, "changed the immutable parent fixture"):
            self.prepare(download)
        self.assertEqual(self.report["registry_cache_preparation"]["status"], "started")

    def test_failed_fetch_is_not_a_pass(self):
        def download():
            raise RuntimeError("unexecuted locked fetch failed")
        with self.assertRaisesRegex(RuntimeError, "fetch failed"):
            self.prepare(download)
        self.assertEqual(self.report["registry_cache_preparation"]["status"], "started")
        self.assertNotIn("source_after_cache_preparation", self.report)


if __name__ == "__main__":
    unittest.main()
