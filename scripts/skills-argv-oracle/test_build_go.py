"""Real Git-tree controls for the fixed Skills oracle's source admission."""
from contextlib import ExitStack
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import textwrap
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
        self.report: dict = {"commands": []}
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

    def preparation(self, graph=None, download=None):
        # Real corrected Git tree, but explicit Go command doubles only. These
        # controls do not constitute a cold-cache build or native evidence.
        self.report["reference"] = BUILDER.CORRECTED
        self.command("checkout", "--detach", BUILDER.source_commit(BUILDER.CORRECTED))
        self.report["source_before"] = BUILDER.source_map(
            self.git, self.source, self.env, self.report)
        env = dict(self.env, GOPROXY="off", GOSUMDB="off", GOENV="off",
                   GOTOOLCHAIN="local", GOWORK="off")
        before = env.copy()
        self.preparation_commands = []
        expected = [["mod", "download"], ["list", "-mod=readonly", "-m", "-json", "all"]]
        actual_run = BUILDER.run

        def command(argv, child_env, report, cwd=None, timeout=120):
            if argv[0] == "unexecuted-go-fixture":
                self.assertEqual(argv[1:], expected[len(self.preparation_commands)])
                self.preparation_commands.append(argv[1:])
                self.assertEqual(cwd, self.source)
                self.assertEqual(timeout, 300)
                self.assertEqual(child_env, dict(before, GOPROXY="https://proxy.golang.org",
                                                 GOSUMDB="sum.golang.org"))
                if argv[1] == "mod":
                    return download(child_env) if download else b""
                return graph(child_env) if graph else b'{"Path":"fixture.invalid/main","Main":true}\n'
            self.assertEqual(child_env, before)
            return actual_run(argv, child_env, report, cwd, timeout)

        try:
            with mock.patch.object(BUILDER, "run", side_effect=command):
                BUILDER.prepare_module_cache("unexecuted-go-fixture", self.git,
                                             self.source, env, self.report)
        finally:
            self.assertEqual(env, before)
        self.assertEqual(self.preparation_commands, expected)

    def test_preparation_keeps_build_environment_offline(self):
        self.preparation()
        self.assertEqual(self.report["module_cache_preparation"]["status"], "passed")
        self.assertEqual(len(self.report["source_before"]), 2439)
        self.assertEqual(self.report["source_before"],
                         self.report["source_after_cache_preparation"])

    def test_preparation_rejects_actual_changed_frozen_source(self):
        def graph(_env):
            source = self.source / "go.sum"
            source.write_bytes(source.read_bytes() + b"\nactual changed source control\n")
            return b'{"Path":"fixture.invalid/main","Main":true}\n'
        with self.assertRaisesRegex(ValueError, "bytes.*committed blob"):
            self.preparation(graph=graph)
        self.assertEqual(self.report["module_cache_preparation"]["status"], "started")

    def test_preparation_rejects_actual_changed_frozen_mode(self):
        def graph(_env):
            if os.name == "nt":
                self.command("update-index", "--chmod=+x", "cmd/symbrain/main.go")
            else:
                (self.source / "cmd/symbrain/main.go").chmod(0o600)
            return b'{"Path":"fixture.invalid/main","Main":true}\n'
        with self.assertRaisesRegex(ValueError, "permissions.*modes|index.*committed tree"):
            self.preparation(graph=graph)
        self.assertEqual(self.report["module_cache_preparation"]["status"], "started")

    def test_preparation_rejects_actual_untracked_source(self):
        def graph(_env):
            (self.source / "unexpected.txt").write_text("owned untracked control\n")
            return b'{"Path":"fixture.invalid/main","Main":true}\n'
        with self.assertRaisesRegex(ValueError, "must remain clean"):
            self.preparation(graph=graph)
        self.assertEqual(self.report["module_cache_preparation"]["status"], "started")

    def test_preparation_failure_is_not_a_pass(self):
        for phase, count in (("download", 1), ("graph", 2)):
            for error in ("module lookup failed", "checksum mismatch"):
                with self.subTest(phase=phase, error=error):
                    def fail(_env):
                        raise RuntimeError(error)
                    with self.assertRaisesRegex(RuntimeError, error):
                        self.preparation(**{phase: fail})
                    self.assertEqual(len(self.preparation_commands), count)
                    self.assertEqual(self.report["module_cache_preparation"]["status"], "started")
                    self.assertNotIn("source_after_cache_preparation", self.report)

    def test_preparation_rejects_empty_partial_or_error_graph(self):
        for raw in (b"", b"{", b"[]", b'{"Error":{"Err":"module lookup failed"}}',
                    b'{"Path":"fixture.invalid/main"}\n{'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                self.preparation(graph=lambda _env: raw)
            self.assertEqual(self.report["module_cache_preparation"]["status"], "started")
            self.assertNotIn("source_after_cache_preparation", self.report)


class BuildOnlyWorkflow(unittest.TestCase):
    """Exercise the actual workflow program, stopping before any SDK command.

    Mocked runner names test routing only, never foreign native execution.
    The real parser writes only failed, explicitly uncompiled test receipts.
    """

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="skills-build-only-control-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.workflow = (BUILDER.ROOT / ".github/workflows/skills-native.yml").read_text()
        step = self.workflow.split("      - name: Produce corrected reference build only\n")
        self.assertEqual(len(step), 2)
        program = step[1].split("        run: |\n", 1)[1].split("      - name:", 1)[0]
        self.program = compile(textwrap.dedent(program), "skills-native.yml:build-only", "exec")
        self.addCleanup(signal.signal, signal.SIGTERM, signal.getsignal(signal.SIGTERM))

    def execute(self, runner="ubuntu-latest", system="Linux", fetch_error=None, build_error=None):
        with ExitStack() as stack:
            stack.enter_context(mock.patch.dict(os.environ, {
                "BUILD_RUNNER": runner, "RUNNER_TEMP": str(self.root),
                "GITHUB_RUN_ID": "unit-uncompiled", "GITHUB_RUN_ATTEMPT": "1"}))
            stack.enter_context(mock.patch.object(platform, "system", return_value=system))
            stack.enter_context(mock.patch.object(sys, "path", sys.path.copy()))
            stack.enter_context(mock.patch.object(sys, "argv", sys.argv.copy()))
            stack.enter_context(mock.patch.object(sys, "dont_write_bytecode", True))
            stack.enter_context(mock.patch.dict(sys.modules, {"build_go": BUILDER}))
            self.fetch = stack.enter_context(mock.patch.object(subprocess, "run", side_effect=fetch_error))
            self.process = stack.enter_context(mock.patch.object(subprocess, "Popen", side_effect=AssertionError("no process allowed")))
            self.build = stack.enter_context(mock.patch.object(BUILDER, "build", side_effect=build_error or RuntimeError("test boundary: no SDK executed")))
            self.admission = stack.enter_context(mock.patch.object(BUILDER.skills_reference, "admit_receipt",
                                                                  side_effect=AssertionError("no admission allowed")))
            exec(self.program, {"__name__": "__main__"})

    def test_workflow_routes_only_explicit_dispatch_to_build_only(self):
        ordinary, manual = self.workflow.split("  corrected-reference-build-only:\n")
        self.assertIn("if: github.event_name != 'workflow_dispatch' || inputs.mode == 'corrected-process'", ordinary)
        self.assertIn("if: github.event_name == 'workflow_dispatch' && (inputs.mode == 'build-only' || inputs.mode == '')", manual)
        self.assertIn("options: [ubuntu-latest, windows-latest]", ordinary)
        self.assertIn("fromJSON(format('[\"{0}\"]', inputs.build_runner))", ordinary)
        self.assertIn("fromJSON('[\"ubuntu-latest\",\"macos-latest\",\"windows-latest\"]')", ordinary)
        self.assertIn("format('{0}-{1}', inputs.mode || 'build-only', inputs.build_runner) || 'process'", ordinary)
        self.assertNotIn("continue-on-error", self.workflow)
        self.assertNotIn("strategy:", manual)
        self.assertEqual(manual.count("        run: |"), 1)
        self.assertIn("shell: python {0}", manual)
        self.assertIn("cache: false", manual)
        self.assertIn("if: always()", manual)
        self.assertIn("if-no-files-found: error", manual)
        self.assertIn("go-build.json", manual)
        self.assertIn("build.*.log", manual)
        self.assertIn("/symbrain*", manual)
        # Existing automatic process gates are still mandatory, not success skips.
        for command in ("go mod download", "cargo fmt --all --check", "cargo clippy --locked",
                        "cargo test --locked", "cargo build --locked", "test_fixture_cleanup.py",
                        "test_full_stdout.py", "test_checkpoint.py", "skills-native-oracle/run.py"):
            self.assertIn(command, ordinary)
        self.assertIn('reference="historical"', ordinary)
        self.assertIn('if [ "$PROCESS_MODE" = corrected-process ]; then', ordinary)
        self.assertIn('reference="skills-control-nofollow-v1"', ordinary)

    def test_actual_workflow_argv_reaches_builder_but_never_comparisons(self):
        # Two declared runner branches, with the real current-host Path semantics.
        for index, (runner, system) in enumerate((("ubuntu-latest", "Linux"), ("windows-latest", "Windows"))):
            with self.subTest(runner=runner):
                self.root = self.root / str(index)
                self.root.mkdir()
                with self.assertRaises(SystemExit) as exited:
                    self.execute(runner, system)
                self.assertEqual(exited.exception.code, 1)
                self.build.assert_called_once()
                args, out, _ = self.build.call_args.args
                self.assertEqual(args.reference, BUILDER.CORRECTED)
                self.assertTrue(args.prepare_module_cache)
                self.assertEqual(args.go_cache, out.parent / "go-cache")
                self.assertEqual(args.module_cache, out.parent / "module-cache")
                self.assertTrue(args.go_cache.is_dir() and args.module_cache.is_dir())
                row = json.loads(args.receipt.read_bytes())
                self.assertEqual(row["reference"], BUILDER.CORRECTED)
                self.assertEqual(row["frozen"], BUILDER.source_commit(BUILDER.CORRECTED))
                self.assertEqual(row["status"], "failed")
                self.assertEqual(row["commands"], [])
                self.assertIn("no SDK executed", row["error"])
                self.assertFalse(out.exists())
                self.assertNotIn("native_surface_gate_passed", row)
                anchor = BUILDER.skills_reference.CORRECTED_ANCHOR
                self.assertEqual(self.fetch.call_args.args[0],
                                 ["git", "fetch", "--no-tags", "origin", f"{anchor}:{anchor}"])
                self.assertTrue(self.fetch.call_args.kwargs["check"])
                self.assertEqual(self.fetch.call_args.kwargs["timeout"], 120)
                self.process.assert_not_called()

    def test_success_exit_stops_before_admission_or_product_comparison(self):
        # Control-flow stub only: do not fabricate a passed receipt or a binary.
        with mock.patch.object(BUILDER, "main", return_value=0) as main, \
                self.assertRaises(SystemExit) as exited:
            self.execute()
        self.assertEqual(exited.exception.code, 0)
        main.assert_called_once_with()
        self.fetch.assert_called_once()
        self.build.assert_not_called()
        self.admission.assert_not_called()
        self.process.assert_not_called()
        self.assertEqual(list(self.root.rglob("go-build.json")), [])

    def test_missing_unknown_darwin_or_mismatched_runner_never_builds(self):
        for runner, system in (("", "Linux"), ("unknown", "Linux"), ("macos-latest", "Darwin"),
                               ("ubuntu-latest", "Darwin"), ("windows-latest", "Linux"),
                               ("ubuntu-latest", "Windows")):
            with self.subTest(runner=runner, system=system), self.assertRaisesRegex(ValueError, "matching Linux/Windows"):
                self.execute(runner, system)
            self.fetch.assert_not_called()
            self.build.assert_not_called()
            self.process.assert_not_called()
        self.assertEqual(list(self.root.iterdir()), [])

    def test_unavailable_source_anchor_never_fetches_or_builds(self):
        with mock.patch.object(BUILDER.skills_reference, "CORRECTED_COMMIT", None), \
                self.assertRaisesRegex(ValueError, "durable reviewed source commit"):
            self.execute()
        self.fetch.assert_not_called()
        self.build.assert_not_called()
        self.assertEqual(list(self.root.iterdir()), [])

    def test_repeated_attempt_never_overwrites_prior_evidence(self):
        with self.assertRaises(SystemExit):
            self.execute()
        before = {p: p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        self.assertTrue(before)
        with self.assertRaises(FileExistsError):
            self.execute()
        self.fetch.assert_not_called()
        self.build.assert_not_called()
        self.assertEqual(before, {p: p.read_bytes() for p in self.root.rglob("*") if p.is_file()})

    def test_fetch_failure_never_becomes_a_build_receipt(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.execute(fetch_error=subprocess.CalledProcessError(23, ["git", "fetch"]))
        self.build.assert_not_called()
        self.process.assert_not_called()
        self.assertEqual(list(self.root.rglob("go-build.json")), [])


if __name__ == "__main__":
    unittest.main()
