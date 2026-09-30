#!/usr/bin/env python3
"""Focused tests for the Browse harness path resolution."""
from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("browse_harness_run", ROOT / "browse/port/harness/run.py")
assert SPEC and SPEC.loader
run = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = run
SPEC.loader.exec_module(run)


class DaemonExitTests(unittest.TestCase):
    def test_race_waits_for_competing_starters_before_stopping_winner(self) -> None:
        winner = Mock(pid=101)
        loser = Mock(pid=102)
        events: list[str] = []
        loser.wait.side_effect = lambda **kwargs: events.append("loser-exited")

        def request(_socket: Path, frame: dict[str, object]) -> dict[str, object]:
            if frame["cmd"] == "daemon.stop":
                self.assertEqual(events, ["loser-exited"])
                events.append("winner-stopped")
            return {"success": True, "data": {"pid": winner.pid}}

        with patch.object(run, "start_daemon", side_effect=[winner, loser]), \
                patch.object(run, "wait_for_path"), patch.object(run, "wait_for_request"), \
                patch.object(run, "request", side_effect=request), \
                patch.object(run, "assert_clean_process") as clean, \
                patch.object(run, "kill_tree"), patch.object(Path, "exists", return_value=False):
            run.race_once(Path("daemon"), {}, Path("runtime"), suffix="test", starters=2)
        self.assertEqual(events, ["loser-exited", "winner-stopped"])
        self.assertEqual(clean.call_count, 2)

    def test_clean_output_does_not_hide_a_failed_daemon(self) -> None:
        for status in (0, 7):
            with self.subTest(status=status), subprocess.Popen(
                [sys.executable, "-c", f"raise SystemExit({status})"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            ) as process:
                if status:
                    with self.assertRaisesRegex(AssertionError, "daemon exited with status 7"):
                        run.assert_clean_process(process)
                else:
                    run.assert_clean_process(process)


class CargoTargetRootTests(unittest.TestCase):
    def test_direct_macos_default_target_is_external(self) -> None:
        root = Path("/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/symaira-brain/browse")
        env: dict[str, str] = {}
        with patch.object(run.sys, "platform", "darwin"), patch.dict(os.environ, {"CI": ""}, clear=False):
            target = run.cargo_target_root(root, env)
        self.assertEqual(target, Path(env[run.EXTERNAL_BASE_ENV]) / "cargo-target")
        self.assertTrue(target.is_relative_to(run.EXTERNAL_RUNTIME_ROOT))

    def test_ci_default_target_stays_in_browse_root(self) -> None:
        root = Path("/workspace/browse")
        with patch.object(run.sys, "platform", "darwin"), patch.dict(os.environ, {"CI": "1"}, clear=False):
            self.assertEqual(run.cargo_target_root(root, {"CI": "1"}), root / "target")

    def test_direct_environment_routes_fetch_outputs_external(self) -> None:
        root = Path("/workspace/browse")
        env: dict[str, str] = {}
        with patch.object(run.sys, "platform", "darwin"), patch.dict(os.environ, {"CI": ""}, clear=False):
            run.external_environment(env)
            report = run.external_output(root, env, "target/fetch002-next/fetch002-e2e.json")
        self.assertTrue(report.is_relative_to(run.EXTERNAL_RUNTIME_ROOT))
        self.assertEqual(env["FETCH_GO_CACHE"], env["GOCACHE"])
        self.assertEqual(env["FETCH_GO_MODCACHE"], env["GOMODCACHE"])
        self.assertTrue(Path(env["GOTELEMETRYDIR"]).is_relative_to(run.EXTERNAL_RUNTIME_ROOT))

    def test_direct_fetch_report_rejects_local_absolute_path(self) -> None:
        env: dict[str, str] = {}
        with patch.object(run.sys, "platform", "darwin"), patch.dict(os.environ, {"CI": ""}, clear=False):
            run.external_environment(env)
            with self.assertRaisesRegex(RuntimeError, "--report must be under"):
                run.external_file(Path("/tmp/fetch-report.json"), env, name="--report")

    def test_ci_report_path_remains_portable(self) -> None:
        path = Path("/tmp/fetch-report.json")
        self.assertEqual(run.external_file(path, {"CI": "1"}, name="--report"), path)

    def test_relative_target_is_resolved_against_browse_root(self) -> None:
        root = Path("/workspace/browse")
        with patch.object(run.sys, "platform", "linux"):
            self.assertEqual(
                run.cargo_target_root(root, {"CARGO_TARGET_DIR": "build/cargo-target"}),
                root / "build/cargo-target",
            )

    def test_absolute_target_is_preserved(self) -> None:
        root = Path("/workspace/browse")
        target = Path("/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/builds/browse-target")
        self.assertEqual(run.cargo_target_root(root, {"CARGO_TARGET_DIR": str(target)}), target)


if __name__ == "__main__":
    unittest.main()
