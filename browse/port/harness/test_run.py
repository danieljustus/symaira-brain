#!/usr/bin/env python3
"""Focused tests for the Browse harness path resolution."""
from __future__ import annotations

import base64
import hashlib
import importlib.util
import json
import os
import sys
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("browse_harness_run", ROOT / "browse/port/harness/run.py")
assert SPEC and SPEC.loader
run = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = run
SPEC.loader.exec_module(run)
CLI_SPEC = importlib.util.spec_from_file_location(
    "browse_cli_differential", ROOT / "browse/port/harness/cli_differential.py"
)
assert CLI_SPEC and CLI_SPEC.loader
cli_differential = importlib.util.module_from_spec(CLI_SPEC)
sys.modules[CLI_SPEC.name] = cli_differential
CLI_SPEC.loader.exec_module(cli_differential)


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

    def test_compat_binary_override_supports_explicit_internal_ci_cache(self) -> None:
        root = Path("/workspace/browse")
        binary = Path("/tmp/symbrain-rescue-target/symbrowse-compat")
        self.assertEqual(
            run.compat_binary_path(root, {"CI": "1", "SYMBROWSE_COMPAT_BINARY": str(binary)}),
            binary,
        )

    def test_direct_macos_compat_override_stays_on_external_volume(self) -> None:
        with patch.object(run.sys, "platform", "darwin"):
            with self.assertRaisesRegex(RuntimeError, "SYMBROWSE_COMPAT_BINARY must be under"):
                run.compat_binary_path(
                    Path("/workspace/browse"),
                    {"SYMBROWSE_COMPAT_BINARY": "/tmp/symbrowse-compat"},
                )

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


class CliDifferentialSessionTests(unittest.TestCase):
    def test_stub_session_matches_both_flag_forms(self) -> None:
        self.assertEqual(
            cli_differential.session_from_argv(["journal", "show", "--session", "fixture"]),
            "fixture",
        )
        self.assertEqual(
            cli_differential.session_from_argv(["journal", "show", "--session=fixture"]),
            "fixture",
        )

    def test_empty_trace_compares_metadata_and_expected_request_independently(self) -> None:
        go = {"cmd": "trace.replay", "session": "fixture", "args": {"steps": []},
              "request_id": "1", "retrieval_surface": "cli"}
        rust = dict(go)
        expected = {"cmd": "trace.replay", "session": "fixture", "args": {"steps": []}}
        self.assertTrue(cli_differential.trace_frames_match([go], [rust], expected))
        self.assertFalse(cli_differential.trace_frames_match([go], [rust], {**expected, "args": {"steps": ["unexpected"]}}))
        self.assertFalse(cli_differential.trace_frames_match([go], [{**rust, "request_id": "2"}], expected))

    def test_completion_oracles_keep_full_native_script_bytes(self) -> None:
        outputs = {shell: f"# {shell}\n".encode() for shell in ("bash", "zsh", "fish", "powershell")}

        def fake_run_process(_binary: Path, argv: list[str], _env: dict[str, str]) -> dict[str, object]:
            return {"returncode": 0, "stdout": outputs[argv[1]], "stderr": b""}

        with patch.object(cli_differential, "run_process", side_effect=fake_run_process):
            rows = cli_differential.completion_oracle_cases(Path("go"), Path("rust"), {})

        self.assertEqual([row["shell"] for row in rows], ["bash", "zsh", "fish", "powershell"])
        for row in rows:
            payload = outputs[row["shell"]]
            self.assertTrue(row["matched"])
            self.assertEqual(base64.b64decode(row["stdout_base64"]), payload)
            self.assertEqual(base64.b64decode(row["rust_stdout_base64"]), payload)
            self.assertEqual(row["stdout_bytes"], len(payload))
            self.assertEqual(row["stdout_sha256"], hashlib.sha256(payload).hexdigest())

    def test_completion_candidate_cases_compare_rust_and_go_protocol_frames(self) -> None:
        calls = []

        def fake_run_process(binary: Path, argv: list[str], _env: dict[str, str]) -> dict[str, object]:
            calls.append((str(binary), argv))
            return {"returncode": 0, "stdout": b"clear\tDelete one cookie\nlist\tList cookies\nset\tSet a cookie\n:4\n", "stderr": b""}

        with patch.object(cli_differential, "run_process", side_effect=fake_run_process):
            rows = cli_differential.completion_candidate_cases(Path("go"), Path("rust"), {})

        self.assertEqual([row["name"] for row in rows], ["cookies-child-commands", "cookies-list-flags"])
        self.assertTrue(all(row["matched"] for row in rows))
        self.assertTrue(all(base64.b64decode(row["go_stdout_base64"]) == base64.b64decode(row["rust_stdout_base64"]) for row in rows))
        self.assertEqual(len(calls), 4)

    def test_out_003_yaml_rows_retain_full_comparison_bytes(self) -> None:
        for argv in (
            ["snapshot", "--session", "fixture", "--output=yaml"],
            ["cookies", "list", "--session", "fixture", "--output=yaml"],
        ):
            with self.subTest(argv=argv):
                outputs = [b"go yaml\n", b"rust yaml\n"]
                with patch.object(
                    cli_differential,
                    "run_process",
                    side_effect=[
                        {"returncode": 0, "stdout": outputs[0], "stderr": b""},
                        {"returncode": 0, "stdout": outputs[1], "stderr": b""},
                    ],
                ):
                    row = cli_differential.compare_processes(
                        Path("go"), Path("rust"), argv, b"", {}, stub=False,
                    )

                self.assertEqual(base64.b64decode(row["go_stdout_base64"]), outputs[0])
                self.assertEqual(base64.b64decode(row["rust_stdout_base64"]), outputs[1])


class CompatSidecarHarnessTests(unittest.TestCase):
    def test_fixture_covers_executable_fetch_002_and_fetch_011_cases(self) -> None:
        fixture = json.loads(
            (ROOT / "browse/port/harness/cases/compat-sidecar.json").read_text()
        )
        cases = {case["id"]: case for case in fixture["cases"]}
        self.assertEqual(fixture["protocol"], 1)
        self.assertEqual(
            cases["compat-six-profiles"]["profiles"],
            ["chrome", "edge", "firefox", "ios", "opera", "safari"],
        )
        for case_id in (
            "compat-pinned-identity",
            "compat-request-id",
            "compat-bounded-frame",
            "compat-integrity-error",
            "compat-typed-fetch-error",
            "compat-timeout-restart",
            "compat-clean-exit",
            "compat-private-endpoint",
        ):
            self.assertIn(case_id, cases)
        self.assertIn("target/compat-sidecar/", cases["compat-rollback-go"]["build"])
        self.assertNotIn("dist/", cases["compat-rollback-go"]["build"])
        self.assertEqual(
            cases["compat-bounded-frame"]["tests"],
            [
                "compat_sidecar_bounds_inbound_ndjson_frames",
                "ndjson_reader_bounds_frames_before_unbounded_growth",
                "outbound_frames_are_bounded_including_the_newline",
            ],
        )
        for case_id, expected_test in {
            "compat-handshake": "production_go_sidecar_exchanges_six_profiles_and_restarts_after_eof",
            "compat-pinned-identity": "production_go_sidecar_exchanges_six_profiles_and_restarts_after_eof",
            "compat-six-profiles": "production_go_sidecar_exchanges_six_profiles_and_restarts_after_eof",
            "compat-request-id": "production_go_sidecar_exchanges_six_profiles_and_restarts_after_eof",
            "compat-typed-fetch-error": "production_go_sidecar_returns_a_typed_fetch_error",
            "compat-timeout-restart": "production_go_sidecar_discards_late_response_after_timeout_restart",
            "compat-clean-exit": "production_go_sidecar_exchanges_six_profiles_and_restarts_after_eof",
            "compat-rollback-go": "production_go_sidecar_exchanges_six_profiles_and_restarts_after_eof",
        }.items():
            self.assertEqual(cases[case_id]["test"], expected_test)
        self.assertEqual(
            cases["compat-integrity-error"]["tests"],
            [
                "handshake_pins_protocol_component_and_oracle",
                "compat_sidecar_rejects_unpinned_handshake_identity",
            ],
        )
        self.assertEqual(
            cases["compat-private-endpoint"]["tests"],
            [
                "private_runtime_directory_schema_and_unix_mode",
                "production_go_sidecar_exchanges_six_profiles_and_restarts_after_eof",
            ],
        )


if __name__ == "__main__":
    unittest.main()
