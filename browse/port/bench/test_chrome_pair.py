import json
import errno
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from chrome_pair import (
    FIXTURE_TITLE, FIXTURE_TOKEN, command, make_env, native_target_matches, nearest_rank,
    remove_owned_tempdir,
    paired_gate_passes, validate_read_output, wait_for_daemon_exit,
)


class ChromePairTests(unittest.TestCase):
    def test_go_and_rust_use_the_same_verified_chrome_launcher(self):
        chrome = Path("/verified/cft/chrome")
        launcher = Path("/verified/cft/chrome-wrapper")
        with tempfile.TemporaryDirectory() as temporary:
            for implementation in ("go", "rust"):
                env = make_env(Path(temporary) / implementation, implementation, chrome, launcher)
                self.assertEqual(env["SYMBROWSE_EXECUTABLE_PATH"], str(launcher))
                self.assertEqual(env["SYMBROWSE_CHROME_EXECUTABLE"], str(launcher))

    def test_daemon_stop_and_status_keep_subcommand_before_session(self):
        binary = Path("symbrowse")
        self.assertEqual(
            command(binary, ["daemon", "stop"], "session"),
            ["symbrowse", "--json", "daemon", "stop", "--session", "session"],
        )
        self.assertEqual(
            command(binary, ["daemon", "status"], "session"),
            ["symbrowse", "--json", "daemon", "status", "--session", "session"],
        )

    def test_read_requires_fixture_title_and_content_token(self):
        good = json.dumps({"success": True, "data": {"title": FIXTURE_TITLE, "markdown": FIXTURE_TOKEN}})
        self.assertTrue(validate_read_output(good))
        self.assertFalse(validate_read_output(json.dumps({"success": True, "data": {"title": FIXTURE_TITLE}})))
        self.assertFalse(validate_read_output("not json"))

    def test_nearest_rank_matches_contract(self):
        self.assertEqual(nearest_rank(list(range(1, 31))), 29)

    def test_paired_gate_needs_complete_samples_and_limits_regression(self):
        go = list(range(100, 130))
        self.assertTrue(paired_gate_passes(go, go, 30))
        self.assertFalse(paired_gate_passes(go, [value * 2 for value in go], 30))
        self.assertFalse(paired_gate_passes(go[:-1], go, 30))

    def test_native_runner_architecture_is_exact_and_win_arm_is_known(self):
        self.assertTrue(native_target_matches("linux-arm64", "Linux", "aarch64"))
        self.assertFalse(native_target_matches("linux-arm64", "Linux", "x86_64"))
        self.assertTrue(native_target_matches("windows-arm64", "Windows", "ARM64"))

    def test_owned_profile_cleanup_retries_transient_chrome_write(self):
        with tempfile.TemporaryDirectory() as parent:
            profile = Path(parent) / "Default"
            profile.mkdir()
            (profile / "Preferences").write_text("fixture", encoding="utf-8")
            attempts = 0

            def remove_after_browser_exit(path):
                nonlocal attempts
                attempts += 1
                if attempts == 1:
                    raise OSError(errno.ENOTEMPTY, "Chrome is still flushing profile data")
                shutil.rmtree(path)

            remove_owned_tempdir(
                profile, timeout=0.2, remove=remove_after_browser_exit, sleep=lambda _: None
            )

            self.assertEqual(attempts, 2)
            self.assertFalse(profile.exists())

    def test_shutdown_waits_until_no_autostart_status_disconnects(self):
        running = (0, json.dumps({"success": True, "data": {"running": True, "pid": 123}}), "")
        stopped = (1, "", "daemon unavailable")
        with patch("chrome_pair.run_cli", side_effect=[running, stopped]) as run_cli:
            self.assertTrue(wait_for_daemon_exit(Path("symbrowse"), "session", {}, Path("."), sleep=lambda _: None))
        self.assertEqual(run_cli.call_count, 2)


if __name__ == "__main__":
    unittest.main()
