"""Real Git-tree controls for the fixed Skills oracle's source admission."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("skills_go_builder", Path(__file__).with_name("build_go.py"))
BUILDER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILDER)


class SourceAdmission(unittest.TestCase):
    def setUp(self):
        os.umask(0o022)
        self.temp = tempfile.TemporaryDirectory(prefix="skills-go-admission-794-")
        self.addCleanup(self.temp.cleanup)
        self.owned = Path(self.temp.name)
        self.source = self.owned / "source"
        home = self.owned / "home"
        home.mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(HOME=str(home), USERPROFILE=str(home), GIT_CONFIG_GLOBAL=os.devnull,
                        GIT_CONFIG_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0")
        self.git = shutil.which("git")
        self.report = {"commands": []}
        self.command("clone", "--shared", "--no-checkout", "--", BUILDER.ROOT, self.source, cwd=None)
        self.command("config", "core.autocrlf", "false")
        self.command("checkout", "--detach", BUILDER.FROZEN)

    def command(self, *args, cwd=True):
        result = subprocess.run([self.git, *map(str, args)], cwd=self.source if cwd else None,
                                env=self.env, capture_output=True, check=False, timeout=120)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def test_owned_real_git_tree_matches_all_frozen_blobs(self):
        self.assertTrue((self.source / ".git").is_dir())
        rows = BUILDER.source_map(self.git, self.source, self.env, self.report)
        self.assertEqual(len(rows), 2438)
        self.assertFalse(self.command("status", "--porcelain"))
        # Ensure actual source bytes and native/index modes, not only HEAD, were checked.
        self.assertTrue(all(row["sha256"] and row["blob"] and row["native_mode"] for row in rows))

    def test_changed_go_payload_is_rejected_before_build(self):
        payload = self.source / "cmd/symbrain/main.go"
        payload.write_bytes(payload.read_bytes() + b"\n// actual owned byte mutant\n")
        with self.assertRaisesRegex(ValueError, "bytes.*committed blob"):
            BUILDER.source_map(self.git, self.source, self.env, self.report)

    def test_changed_index_mode_is_rejected_on_every_platform(self):
        self.command("update-index", "--chmod=+x", "cmd/symbrain/main.go")
        with self.assertRaisesRegex(ValueError, "index.*committed tree"):
            BUILDER.source_map(self.git, self.source, self.env, self.report)

    def test_output_never_overwrites_prior_binary_or_source(self):
        existing = self.owned / "prior-go"
        existing.write_bytes(b"prior immutable payload")
        with self.assertRaisesRegex(ValueError, "overwrite"):
            BUILDER.output_path(existing)
        with self.assertRaisesRegex(ValueError, "outside the checkout"):
            BUILDER.output_path(BUILDER.ROOT / "never-created-go")
        self.assertEqual(existing.read_bytes(), b"prior immutable payload")

    def preparation(self, download):
        self.report["source_before"] = BUILDER.source_map(
            self.git, self.source, self.env, self.report)
        env = dict(self.env, GOPROXY="off", GOSUMDB="off")
        actual_run = BUILDER.run

        def command(argv, child_env, report, cwd=None, timeout=120):
            if argv[0] == "unexecuted-go-fixture":
                self.assertEqual(argv[1:], ["mod", "download"])
                self.assertEqual(timeout, 300)
                return download(child_env)
            return actual_run(argv, child_env, report, cwd, timeout)

        with mock.patch.object(BUILDER, "run", side_effect=command):
            BUILDER.prepare_module_cache("unexecuted-go-fixture", self.git,
                                         self.source, env, self.report)
        self.assertEqual(env["GOPROXY"], "off")
        self.assertEqual(env["GOSUMDB"], "off")

    def test_preparation_keeps_build_environment_offline(self):
        def download(env):
            self.assertEqual(env["GOPROXY"], "https://proxy.golang.org")
            self.assertEqual(env["GOSUMDB"], "sum.golang.org")
            return b""
        self.preparation(download)
        self.assertEqual(self.report["module_cache_preparation"]["status"], "passed")
        self.assertEqual(self.report["source_before"],
                         self.report["source_after_cache_preparation"])

    def test_preparation_rejects_actual_changed_frozen_source(self):
        def download(_env):
            source = self.source / "go.sum"
            source.write_bytes(source.read_bytes() + b"\nactual changed source control\n")
            return b""
        with self.assertRaisesRegex(ValueError, "bytes.*committed blob"):
            self.preparation(download)
        self.assertEqual(self.report["module_cache_preparation"]["status"], "started")

    def test_preparation_failure_is_not_a_pass(self):
        def download(_env):
            raise RuntimeError("owned unexecuted SDK download failed")
        with self.assertRaisesRegex(RuntimeError, "download failed"):
            self.preparation(download)
        self.assertEqual(self.report["module_cache_preparation"]["status"], "started")
        self.assertNotIn("source_after_cache_preparation", self.report)


if __name__ == "__main__":
    unittest.main()
