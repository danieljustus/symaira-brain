import json
import errno
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from chrome_pair import (
    FIXTURE_TITLE, FIXTURE_TOKEN, MAX_OUTPUT, command, make_env, native_target_matches, nearest_rank,
    remove_owned_tempdir, summarize_stage,
    paired_gate_passes, validate_read_output, wait_for_daemon_exit, flow, measure, run_cli,
)


class ChromePairTests(unittest.TestCase):
    def test_run_cli_keeps_the_decoded_output_limit(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch("chrome_pair.command", return_value=[
                sys.executable, "-c", "import sys; sys.stdout.write('x' * (1 << 20) + '!')"
            ]):
                result = run_cli(Path("unused"), ["open", "http://127.0.0.1/"], "s", {}, Path(temporary))
        self.assertEqual(result, (1, "", "output limit exceeded"))

    def test_run_cli_preserves_text_output_and_exit_status(self):
        with tempfile.TemporaryDirectory() as temporary:
            script = "import sys; sys.stdout.write('line\\r\\n'); sys.stderr.write('warning\\r\\n'); sys.exit(7)"
            with patch("chrome_pair.command", return_value=[sys.executable, "-c", script]):
                result = run_cli(Path("unused"), ["open", "http://127.0.0.1/"], "s", {}, Path(temporary))
        self.assertEqual(result, (7, "line\n", "warning\n"))

    def test_timeout_does_not_wait_for_descendant_inheriting_capture_handles(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pid_file = root / "descendant.pid"
            parent = (
                "import pathlib,subprocess,sys,time; "
                "child=subprocess.Popen([sys.executable,'-c','import time;time.sleep(8)']); "
                "pathlib.Path(sys.argv[1]).write_text(str(child.pid)); time.sleep(30)"
            )
            started = time.monotonic()
            try:
                with patch("chrome_pair.command", return_value=[sys.executable, "-c", parent, str(pid_file)]):
                    with self.assertRaises(subprocess.TimeoutExpired):
                        run_cli(Path("unused"), ["open", "http://127.0.0.1/"], "s", {}, root, timeout=0.5)
                self.assertLess(time.monotonic() - started, 3.0)
                self.assertTrue(pid_file.is_file(), "the fixture descendant must be started before timeout")
            finally:
                if pid_file.is_file():
                    try:
                        os.kill(int(pid_file.read_text(encoding="ascii")), signal.SIGTERM)
                    except (OSError, ValueError):
                        pass

    def test_cleanup_failure_preserves_the_primary_open_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            response = json.dumps({"error": {"code": "operation_timeout"}})
            with patch("chrome_pair.run_cli", side_effect=[
                (1, response, ""), subprocess.TimeoutExpired("daemon stop", 45),
            ]):
                result = flow(root / "symbrowse", "rust", root / "chrome", None,
                              "http://127.0.0.1/", root, 0)
            self.assertEqual(result["phase"], "open")
            self.assertEqual(result["error_code"], "operation_timeout")
            self.assertIn("cleanup_error", result)

    def test_rust_open_failure_reports_only_known_stages_from_daemon_log(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            response = json.dumps({"error": {"code": "operation_timeout"}})

            def run(binary, args, session, env, cwd, **kwargs):
                if args == ["open", "http://127.0.0.1/"]:
                    return 1, response, ""
                if args == ["daemon", "stop"]:
                    (root / "daemon.log").write_text(
                        "warning contains a secret\n"
                        "chrome_open_stage=main-frame status=missing\n"
                        "chrome_open_stage=navigation-future-start\n"
                        "chrome_open_stage=navigation-future status=timeout\n"
                        "chrome_open_stage=unknown-secret\n"
                        "chrome_open_stage=document-probe status=timeout\n"
                        "chrome_open_stage=navigation-event-wait-start\n",
                        encoding="utf-8",
                    )
                    return 0, "", ""
                return 1, "", "daemon stopped"

            with patch("chrome_pair.run_cli", side_effect=run):
                result = flow(Path("symbrowse"), "rust", Path("chrome"), None,
                              "http://127.0.0.1/", root, 0)

        self.assertEqual(result["error_code"], "operation_timeout")
        self.assertEqual(result["chrome_open_stages"], [
            "main-frame status=missing", "navigation-future-start",
            "navigation-future status=timeout",
            "document-probe status=timeout", "navigation-event-wait-start",
        ])

    def test_failed_open_records_code_and_stops_unusable_benchmark(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "symbrowse"
            chrome = root / "chrome"
            for path in (binary, chrome):
                path.write_bytes(b"fixture")
                path.chmod(0o700)
            response = json.dumps({"success": False, "error": {"code": "navigation", "message": "fixture unavailable"}})
            with patch("chrome_pair.run_cli", side_effect=[(1, response, ""), (0, "", ""), (1, "", "")]):
                result = flow(binary, "go", chrome, None, "http://127.0.0.1/", root, 0)
            self.assertEqual((result["error_code"], result["error_message"]), ("navigation", "fixture unavailable"))

            args = SimpleNamespace(target="linux-amd64", repo=Path(__file__).resolve().parents[3],
                                   expected_source_revision=None, go=binary, rust=binary, chrome=chrome,
                                   chrome_version="fixture", chrome_archive_sha256="a" * 64,
                                   chrome_launcher=None, runs=30)
            with patch("chrome_pair.native_target_matches", return_value=True), patch("chrome_pair.flow", return_value=result) as run_flow:
                report = measure(args)
            self.assertEqual(run_flow.call_count, 2)
            self.assertEqual(report["gate"], "blocked")

    def test_measure_reports_cleanup_failure_without_losing_open_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "symbrowse"
            chrome = root / "chrome"
            for path in (binary, chrome):
                path.write_bytes(b"fixture")
                path.chmod(0o700)
            sample = {"status": "error", "phase": "open", "error_code": "operation_timeout"}
            args = SimpleNamespace(target="windows-amd64", repo=Path(__file__).resolve().parents[3],
                                   expected_source_revision=None, go=binary, rust=binary, chrome=chrome,
                                   chrome_version="fixture", chrome_archive_sha256="a" * 64,
                                   chrome_launcher=None, runs=30)
            with patch("chrome_pair.native_target_matches", return_value=True), \
                 patch("chrome_pair.flow", return_value=sample), \
                 patch("chrome_pair.remove_owned_tempdir", side_effect=PermissionError(errno.EACCES, "sharing violation")) as cleanup:
                report = measure(args)

        self.assertEqual(report["status"], "error")
        self.assertEqual(report["reason"], "owned Chrome profile cleanup failed; see sample cleanup_error")
        self.assertEqual(cleanup.call_args.kwargs["timeout"], 30.0)
        samples = [item for binary_report in report["binaries"].values()
                   for item in binary_report["chrome_flow"]["samples"]]
        self.assertEqual(len(samples), 1)
        self.assertEqual(samples[0]["error_code"], "operation_timeout")
        self.assertIn("cleanup_error", samples[0])

    def test_go_and_rust_use_the_same_verified_chrome_launcher(self):
        chrome = Path("/verified/cft/chrome")
        launcher = Path("/verified/cft/chrome-wrapper")
        with tempfile.TemporaryDirectory() as temporary:
            for implementation in ("go", "rust"):
                env = make_env(Path(temporary) / implementation, implementation, chrome, launcher)
                self.assertEqual(env["SYMBROWSE_EXECUTABLE_PATH"], str(launcher))
                self.assertEqual(env["SYMBROWSE_CHROME_EXECUTABLE"], str(launcher))
                if implementation == "rust":
                    self.assertEqual(env["SYMBROWSE_E2E"], "1")
                    self.assertEqual(env["SYMBROWSE_DAEMON_LOG"], str(Path(temporary) / implementation / "daemon.log"))

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

    def test_flow_reports_open_and_read_cli_stage_durations(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            response = json.dumps({"success": True, "data": {"title": FIXTURE_TITLE, "markdown": FIXTURE_TOKEN}})
            with patch("chrome_pair.run_cli", side_effect=[
                (0, "{}", ""), (0, response, ""), (0, "", ""), (1, "", ""),
            ]), patch("chrome_pair.time.perf_counter_ns", side_effect=[100, 110, 160, 165, 200, 205]):
                result = flow(root / "symbrowse", "go", root / "chrome", None,
                              "http://127.0.0.1/fixture.html", root, 0)

        self.assertEqual(result["status"], "pass")
        self.assertEqual(result["duration_ns"], 105)
        self.assertEqual(result["open_cli_duration_ns"], 50)
        self.assertEqual(result["read_cli_duration_ns"], 35)

    def test_stage_summary_is_diagnostic_and_uses_same_nearest_rank(self):
        samples = [
            {"status": "pass", "open_cli_duration_ns": duration}
            for duration in range(1, 31)
        ]
        self.assertEqual(
            summarize_stage(samples, "open_cli_duration_ns"),
            {"status": "complete", "samples": 30, "median_duration_ns": 15,
             "p95_duration_ns": 29},
        )
        samples[-1] = {"status": "error", "open_cli_duration_ns": 100}
        self.assertEqual(
            summarize_stage(samples, "open_cli_duration_ns"),
            {"status": "incomplete", "samples": 29, "median_duration_ns": 15,
             "p95_duration_ns": 28},
        )

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
