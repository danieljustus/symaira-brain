"""Real Git-tree controls for the fixed Skills oracle's source admission."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
