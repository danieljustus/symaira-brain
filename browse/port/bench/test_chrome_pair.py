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
    paired_gate_passes, read_output_markers, validate_read_output,
    wait_for_daemon_exit, daemon_startup_log, windows_profile_process_count,
    wait_for_windows_profile_cleanup, flow, measure, run_cli,
)


class ChromePairTests(unittest.TestCase):
    def test_rust_environment_enables_diagnostics_only_for_the_unmeasured_retry(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            ordinary = make_env(root / "ordinary", "rust", root / "chrome")
            diagnostic = make_env(root / "diagnostic", "rust", root / "chrome", diagnostics=True)

        self.assertNotIn("SYMBROWSE_PERF_DIAGNOSTICS", ordinary)
        self.assertEqual(diagnostic["SYMBROWSE_PERF_DIAGNOSTICS"], "1")
        self.assertEqual(
            diagnostic["SYMBROWSE_DAEMON_LOG"],
            str(root / "diagnostic" / "home" / ".local" / "state" / "symbrowse" / "daemon.log"),
        )

    def test_windows_profile_process_probe_returns_a_count_without_command_lines(self):
        output = json.dumps([
            {"name": "chrome.exe", "count": 2},
            {"name": "crashpad_handler.exe", "count": 1},
        ])
        with patch("chrome_pair.subprocess.run", return_value=SimpleNamespace(stdout=output)) as run:
            count = windows_profile_process_count(Path("D:/runner/sample-profile"))

        self.assertEqual(count, 3)
        self.assertIn("Get-CimInstance Win32_Process", run.call_args.args[0][-1])
        self.assertEqual(run.call_args.kwargs["timeout"], 5)

    def test_windows_profile_cleanup_probe_has_time_for_process_enumeration(self):
        with patch("chrome_pair.windows_profile_process_count", return_value=0) as count:
            self.assertEqual(wait_for_windows_profile_cleanup(Path("D:/runner/sample-profile")), 0)

        self.assertGreaterEqual(count.call_args.kwargs["timeout"], 14.9)

    def test_rust_flow_fails_when_chrome_keeps_the_stopped_profile_open(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            response = json.dumps({"success": True, "data": {"title": FIXTURE_TITLE, "markdown": FIXTURE_TOKEN}})
            with patch("chrome_pair.run_cli", side_effect=[
                (0, "{}", ""), (0, response, ""), (0, "", ""),
                (0, json.dumps({"success": True, "data": {"running": False}}), ""),
            ]), patch("chrome_pair.sys.platform", "win32"), \
                 patch("chrome_pair.wait_for_windows_profile_cleanup", return_value=2), \
                 patch("chrome_pair.daemon_startup_log", return_value="symbrowse shutdown: browser close closed"):
                result = flow(root / "symbrowse", "rust", root / "chrome", None,
                              "http://127.0.0.1/fixture.html", root, 0)

        self.assertEqual(result["status"], "error")
        self.assertEqual(result["phase"], "chrome-cleanup")
        self.assertEqual(result["chrome_profile_processes_after_stop"], 2)
        self.assertIn("browser close closed", result["daemon_shutdown_log"])

    def test_rust_flow_preserves_shutdown_log_when_process_probe_times_out(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            response = json.dumps({"success": True, "data": {"title": FIXTURE_TITLE, "markdown": FIXTURE_TOKEN}})
            with patch("chrome_pair.run_cli", side_effect=[
                (0, "{}", ""), (0, response, ""), (0, "", ""),
                (0, json.dumps({"success": True, "data": {"running": False}}), ""),
            ]), patch("chrome_pair.sys.platform", "win32"), \
                 patch("chrome_pair.wait_for_windows_profile_cleanup",
                       side_effect=subprocess.TimeoutExpired("powershell.exe", 15)), \
                 patch("chrome_pair.daemon_startup_log", return_value="symbrowse shutdown: browser close close-timeout"):
                result = flow(root / "symbrowse", "rust", root / "chrome", None,
                              "http://127.0.0.1/fixture.html", root, 0)

        self.assertEqual(result["status"], "error")
        self.assertEqual(result["phase"], "chrome-cleanup")
        self.assertEqual(result["chrome_cleanup_probe_error"], "TimeoutExpired after 15s")
        self.assertIn("browser close close-timeout", result["daemon_shutdown_log"])

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

    def test_rust_open_failure_captures_navigation_log_even_after_chrome_exit(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            response = json.dumps({"error": {"code": "operation_timeout"}})
            with patch("chrome_pair.run_cli", side_effect=[
                (1, response, ""), (0, "", ""),
                (0, json.dumps({"success": True, "data": {"running": False}}), ""),
            ]), patch("chrome_pair.sys.platform", "win32"), \
                 patch("chrome_pair.wait_for_windows_profile_cleanup", return_value=0), \
                 patch("chrome_pair.daemon_startup_log",
                       return_value="symbrowse Chrome navigation: state poll=20 ready_state=loading") as log:
                result = flow(root / "symbrowse", "rust", root / "chrome", None,
                              "http://127.0.0.1/fixture.html", root, 0)

        self.assertEqual(result["error_code"], "operation_timeout")
        self.assertEqual(result["chrome_profile_processes_after_stop"], 0)
        self.assertIn("ready_state=loading", result["daemon_navigation_log"])
        self.assertEqual(log.call_count, 1)

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

    def test_daemon_startup_failure_captures_bounded_redacted_child_log(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "symbrowse"
            chrome = root / "chrome"
            for path in (binary, chrome):
                path.write_bytes(b"fixture")
                path.chmod(0o700)
            response = json.dumps({"error": {"code": "daemon_unavailable", "message": "daemon did not become ready"}})

            def fake_cli(_binary, args, _session, env, _cwd, **_kwargs):
                if args[0] == "open":
                    log = Path(env["XDG_STATE_HOME"]) / "symbrowse" / "daemon.log"
                    log.parent.mkdir(parents=True)
                    log.write_text("bind failed: address unavailable token=private-value\n", encoding="utf-8")
                    return 1, response, ""
                return 1, "", ""

            with patch("chrome_pair.run_cli", side_effect=fake_cli):
                result = flow(binary, "go", chrome, None, "http://127.0.0.1/", root, 0)

        self.assertEqual(result["error_code"], "daemon_unavailable")
        self.assertIn("bind failed: address unavailable", result["daemon_startup_log"])
        self.assertIn("token=[redacted]", result["daemon_startup_log"])
        self.assertNotIn("private-value", result["daemon_startup_log"])

    def test_daemon_startup_log_keeps_only_the_bounded_tail(self):
        with tempfile.TemporaryDirectory() as temporary:
            state_home = Path(temporary)
            log = state_home / "symbrowse" / "daemon.log"
            log.parent.mkdir(parents=True)
            log.write_text("a" * 32 + "tail", encoding="utf-8")

            diagnostic = daemon_startup_log({"XDG_STATE_HOME": str(state_home)}, limit=8)

        self.assertEqual(diagnostic, "[earlier daemon log bytes omitted]\n" + "a" * 4 + "tail")

    def test_operation_timeout_gets_one_unmeasured_diagnostic_retry(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "symbrowse"
            chrome = root / "chrome"
            for path in (binary, chrome):
                path.write_bytes(b"fixture")
                path.chmod(0o700)
            args = SimpleNamespace(target="linux-amd64", repo=Path(__file__).resolve().parents[3],
                                   expected_source_revision=None, go=binary, rust=binary, chrome=chrome,
                                   chrome_version="fixture", chrome_archive_sha256="a" * 64,
                                   chrome_launcher=None, runs=1)
            go_pass = {"status": "pass", "duration_ns": 100}
            rust_timeout = {"status": "error", "error_code": "operation_timeout"}
            diagnostic_pass = {"status": "pass", "diagnostic_daemon_log": "navigation.dispatch.start"}
            with patch("chrome_pair.native_target_matches", return_value=True),                  patch("chrome_pair.random.Random", return_value=SimpleNamespace(randrange=lambda _size: 0)),                  patch("chrome_pair.flow", side_effect=[go_pass, rust_timeout, diagnostic_pass]) as run_flow:
                report = measure(args)

        self.assertEqual(run_flow.call_count, 3)
        self.assertNotIn("diagnostics", run_flow.call_args_list[1].kwargs)
        self.assertTrue(run_flow.call_args.kwargs["diagnostics"])
        self.assertNotIn("diagnostic_only", report)
        self.assertTrue(report["windows_amd64_operation_timeout_diagnostic"]["diagnostic_only"])
        self.assertEqual(report["windows_amd64_operation_timeout_diagnostic"]["diagnostic_daemon_log"],
                         "navigation.dispatch.start")
        self.assertEqual(report["binaries"]["rust"]["chrome_flow"]["samples"], [rust_timeout])
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

    def test_commands_place_json_after_subcommand(self):
        binary = Path("symbrowse")
        self.assertEqual(
            command(binary, ["daemon", "stop"], "session"),
            ["symbrowse", "daemon", "stop", "--json", "--session", "session"],
        )
        self.assertEqual(
            command(binary, ["daemon", "status"], "session"),
            ["symbrowse", "daemon", "status", "--json", "--session", "session"],
        )
        self.assertEqual(
            command(binary, ["get", "url"], "session"),
            ["symbrowse", "get", "--json", "--session", "session", "url"],
        )
        self.assertEqual(
            command(binary, ["read"], "session"),
            ["symbrowse", "read", "--json", "--session", "session"],
        )

    def test_read_requires_fixture_title_and_content_token(self):
        good = json.dumps({"success": True, "data": {"title": FIXTURE_TITLE, "markdown": FIXTURE_TOKEN}})
        self.assertTrue(validate_read_output(good))
        self.assertEqual(read_output_markers(good), {"fixture_title_present": True, "fixture_token_present": True})
        self.assertEqual(
            read_output_markers(json.dumps({"success": True, "data": {"markdown": FIXTURE_TOKEN}})),
            {"fixture_title_present": False, "fixture_token_present": True},
        )
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
