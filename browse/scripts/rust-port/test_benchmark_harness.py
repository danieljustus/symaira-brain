#!/usr/bin/env python3
"""Regression tests for the paired benchmark daemon command contract."""
from __future__ import annotations

import importlib.util
import os
import select
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("bench_run", ROOT / "port/bench/run.py")
assert SPEC and SPEC.loader
bench_run = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bench_run
SPEC.loader.exec_module(bench_run)


class BenchmarkHarnessTests(unittest.TestCase):
    @unittest.skipUnless(
        bench_run.EXTERNAL_RUNTIME_ROOT.is_mount(), "local NVMe volume is unavailable"
    )
    def test_macos_temp_parent_requires_external_writable_root(self) -> None:
        with patch.object(bench_run.platform, "system", return_value="Darwin"):
            with patch.dict(os.environ, {"CI": ""}, clear=False):
                os.environ.pop(bench_run.EXTERNAL_RUNTIME_ENV, None)
                with self.assertRaisesRegex(RuntimeError, bench_run.EXTERNAL_RUNTIME_ENV):
                    bench_run.temporary_parent()
                with tempfile.TemporaryDirectory(dir=bench_run.EXTERNAL_RUNTIME_ROOT) as tmp:
                    os.environ[bench_run.EXTERNAL_RUNTIME_ENV] = tmp
                    self.assertEqual(bench_run.temporary_parent(), str(Path(tmp).resolve()))
                    os.environ[bench_run.EXTERNAL_RUNTIME_ENV] = "/private/tmp"
                    with self.assertRaisesRegex(RuntimeError, "under mounted NVMe volume"):
                        bench_run.temporary_parent()
                    os.environ[bench_run.EXTERNAL_RUNTIME_ENV] = str(Path(tmp) / "missing")
                    with self.assertRaisesRegex(RuntimeError, bench_run.EXTERNAL_RUNTIME_ENV):
                        bench_run.temporary_parent()

    def test_ci_and_non_macos_keep_portable_temp_parent(self) -> None:
        with patch.object(bench_run.platform, "system", return_value="Darwin"):
            with patch.dict(os.environ, {"CI": "1"}, clear=False):
                self.assertIsNone(bench_run.temporary_parent())

    @unittest.skipUnless(
        bench_run.EXTERNAL_RUNTIME_ROOT.is_mount(), "local NVMe volume is unavailable"
    )
    def test_macos_output_must_stay_on_nvme(self) -> None:
        with patch.object(bench_run.platform, "system", return_value="Darwin"):
            with tempfile.TemporaryDirectory(dir=bench_run.EXTERNAL_RUNTIME_ROOT) as tmp:
                with patch.dict(
                    os.environ,
                    {"CI": "", bench_run.EXTERNAL_RUNTIME_ENV: tmp},
                    clear=False,
                ):
                    self.assertEqual(
                        bench_run.external_output(Path(tmp) / "report.json"),
                        Path(tmp).resolve() / "report.json",
                    )
                    with self.assertRaisesRegex(RuntimeError, "--output must be under mounted NVMe volume"):
                        bench_run.external_output(Path("/private/tmp/report.json"))
        with patch.object(bench_run.platform, "system", return_value="Linux"):
            with patch.dict(os.environ, {}, clear=False):
                self.assertIsNone(bench_run.temporary_parent())

    def test_static_daemon_command_matches_each_cli_contract(self) -> None:
        binary = Path(tempfile.gettempdir()) / "symbrowse"
        self.assertEqual(
            bench_run.daemon_command(binary, "go", static_mode=False),
            [str(binary), "daemon", "--session", "go", "--engine", "static"],
        )
        self.assertEqual(
            bench_run.daemon_command(binary, "rust", static_mode=True),
            [str(binary), "daemon", "--session", "rust", "--mode", "static"],
        )
        self.assertNotIn("--engine", bench_run.daemon_command(binary, "rust", static_mode=True))

    def test_environment_selection_is_implementation_specific(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            go = bench_run.implementation_env(root, "go")
            rust = bench_run.implementation_env(root, "rust")
            self.assertEqual(go["SYMBROWSE_ENGINE"], "static")
            self.assertNotIn("SYMBROWSE_MODE", go)
            self.assertEqual(rust["SYMBROWSE_MODE"], "browser")
            self.assertNotIn("SYMBROWSE_ENGINE", rust)

    @unittest.skipUnless(os.name == "posix", "Unix process-group probe")
    def test_startup_failure_is_bounded_and_kills_descendant(self) -> None:
        # A ready, SIGTERM-resistant child owns the pipe. EOF proves this
        # specific descendant stopped, without inspecting unrelated processes.
        child = "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); print('ready', flush=True); time.sleep(30)"
        for leader_exits in (False, True):
            with self.subTest(leader_exits=leader_exits), tempfile.TemporaryDirectory() as tmp:
                log = Path(tmp) / "startup.log"
                code = f"import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{child!r}]); "
                code += "sys.exit(0)" if leader_exits else "time.sleep(30)"
                with log.open("wb") as stderr:
                    process = subprocess.Popen([sys.executable, "-c", code], stdout=subprocess.PIPE,
                                               stderr=stderr, start_new_session=True)
                assert process.stdout is not None
                try:
                    self.assertTrue(select.select([process.stdout], [], [], 5)[0], "child not ready")
                    self.assertEqual(process.stdout.readline(), b"ready\n")
                    if leader_exits:
                        process.wait(timeout=3)
                    started = time.monotonic()
                    result = bench_run.startup_failure(process, "startup failed", log)
                    self.assertLess(time.monotonic() - started, 3)
                    self.assertEqual(result["status"], "error")
                    self.assertIsNotNone(process.poll())
                    self.assertTrue(select.select([process.stdout], [], [], 3)[0], "child retained pipe")
                    self.assertEqual(process.stdout.read(1), b"")
                finally:
                    bench_run.terminate_process_tree(process)
                    process.stdout.close()

    def test_launch_failure_closes_descriptor_and_removes_log(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = bench_run.base_env(root)
            descriptors = []
            real_mkstemp = tempfile.mkstemp

            def tracked_mkstemp(*args, **kwargs):
                fd, name = real_mkstemp(*args, **kwargs)
                descriptors.append(fd)
                return fd, name

            with patch.object(bench_run.tempfile, "mkstemp", side_effect=tracked_mkstemp):
                with self.assertRaises(FileNotFoundError):
                    bench_run.launch_daemon([str(root / "missing-binary")], root, env)
            self.assertEqual(list((root / "tmp").iterdir()), [])
            for fd in descriptors:
                with self.assertRaises(OSError):
                    os.fstat(fd)


if __name__ == "__main__":
    unittest.main()
