"""Noncompiling controls; explicit reviewed-file mode is admission, not parity."""
import argparse
from contextlib import ExitStack
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import run
from ordinary_startup import BINDINGS, bind, pair, using_reference
from fixtures import PROFILE, write
from test_startup_controls import synthetic_pair
import skills_reference as reference
import build_go
import compiler_inputs


class FixtureBoundary(RuntimeError):
    pass


class ReferenceAdmission(unittest.TestCase):
    """Synthetic unit controls only, never native/compiler evidence."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="skills-reference-admission-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "never-executed-synthetic-binary"
        self.binary.write_bytes(b"synthetic test bytes, not an executable")
        self.receipt = self.root / "synthetic-build.json"
        self.out = self.root / "must-not-pass.json"
        self.historical = {"status": "passed", "frozen": reference.FROZEN,
                           "output_sha256": hashlib.sha256(self.binary.read_bytes()).hexdigest()}
        self.save(self.historical)
        self.argv = ["run.py", "--go", str(self.binary), "--rust", str(self.binary),
                     "--git", str(self.binary), "--git-fixture", str(self.binary),
                     "--oracle-receipt", str(self.receipt), "--out", str(self.out)]
        write(self.root / "profile.toml", PROFILE)
        self.addCleanup(signal.signal, signal.SIGTERM, signal.getsignal(signal.SIGTERM))

    def save(self, row):
        self.receipt.write_text(json.dumps(row))
        return hashlib.sha256(self.receipt.read_bytes()).hexdigest()

    def synthetic_anchor(self, stack):
        # Explicitly isolated test-only trust root, not a production pin/receipt.
        commit = "c" * 40
        rows = [{"path_bytes_hex": b"synthetic.go".hex(), "git_mode": "100644", "blob": "d" * 40}]
        stack.enter_context(patch.object(reference, "CORRECTED_COMMIT", commit))
        stack.enter_context(patch.object(reference, "CORRECTED_ANCHOR", "refs/tags/synthetic-test-only"))
        stack.enter_context(patch.object(reference, "CORRECTED_TREE", reference.tree_identity(rows)))
        inputs = {field: {"synthetic": True} for field in ("files", "sdk_files", "modules", "go_env")}
        row = {**self.historical, "reference": reference.CORRECTED, "frozen": commit,
               "toolchain": reference.TOOLCHAIN, "source_before": rows, "source_after": deepcopy(rows),
               "compiler_inputs_before": inputs, "compiler_inputs_after": deepcopy(inputs)}
        digest = self.save(row)
        anchors = {(reference.sys.platform, reference.platform.machine()): digest}
        stack.enter_context(patch.object(reference, "CORRECTED_RECEIPTS", anchors))
        return row, anchors

    def test_actual_builder_refuses_pending_reference_without_any_command(self):
        argv = ["build_go.py", "--reference", reference.CORRECTED,
                "--out", str(self.root / "unbuilt"), "--receipt", str(self.root / "failed-build.json"),
                "--go-cache", str(self.root), "--module-cache", str(self.root)]
        with patch.object(reference, "CORRECTED_COMMIT", None), \
                patch.object(sys, "argv", argv), patch.object(build_go, "run") as command:
            self.assertEqual(build_go.main(), 1)
        command.assert_not_called()
        receipt = json.loads((self.root / "failed-build.json").read_bytes())
        self.assertEqual(receipt["status"], "failed")
        self.assertEqual(receipt["commands"], [])
        self.assertFalse((self.root / "unbuilt").exists())

    def test_source_anchor_alone_does_not_admit_native_execution(self):
        commit = reference.source_commit(reference.CORRECTED)
        self.assertNotEqual(commit, reference.FROZEN)
        self.assertEqual(reference.CORRECTED_ANCHOR, "refs/heads/oracle/skills-control-nofollow-v1")
        # Source retention is not receipt approval, even with matching claimed fields.
        self.save({**self.historical, "frozen": commit, "reference": reference.CORRECTED})
        with patch.object(reference, "CORRECTED_RECEIPTS", {}), \
                patch.object(sys, "argv", self.argv + ["--reference", reference.CORRECTED]), \
                patch.object(run, "invoke") as invoke, patch.object(run, "setup") as setup:
            with self.assertRaisesRegex(ValueError, "independently anchored native build provenance"):
                run.main()
        invoke.assert_not_called()
        setup.assert_not_called()
        self.assertFalse(self.out.exists())

    def test_build_only_receipts_cannot_admit_unregistered_platform_or_unreviewed_bytes(self):
        # Routing simulations only: no native execution, synthetic success claim
        # intentionally rejected by the real run.py entrypoint and unchanged pins.
        pins = dict(reference.CORRECTED_RECEIPTS)
        self.save({**self.historical, "reference": reference.CORRECTED,
                   "frozen": reference.source_commit(reference.CORRECTED)})
        for system, machine, diagnostic in (
                ("linux", "x86_64", "independently reviewed"),
                ("win32", "AMD64", "independently reviewed"),
                ("linux", "amd64", "independently anchored"),
                ("win32", "x86_64", "independently anchored"),
                ("unknown", "unknown", "independently anchored"),
                ("darwin", "arm64", "independently reviewed")):
            with self.subTest(system=system), \
                    patch.object(reference.sys, "platform", system), \
                    patch.object(reference.platform, "machine", return_value=machine), \
                    patch.object(sys, "argv", self.argv + ["--reference", reference.CORRECTED]), \
                    patch.object(run, "invoke") as invoke, patch.object(run, "setup") as setup, \
                    patch.object(subprocess, "Popen", side_effect=AssertionError("process forbidden")) as process:
                with self.assertRaisesRegex(ValueError, diagnostic):
                    run.main()
                invoke.assert_not_called()
                setup.assert_not_called()
                process.assert_not_called()
                self.assertFalse(self.out.exists())
        self.assertEqual(reference.CORRECTED_RECEIPTS, pins)

    def test_reviewed_fetch_rejects_wrong_route_and_missing_artifact(self):
        # Routing simulation and failed transport only; no synthetic trust pin.
        for key, approved in reference.REVIEWED_ARTIFACTS.items():
            with self.subTest(key=key), ExitStack() as stack:
                stack.enter_context(patch.object(reference.sys, "platform", key[0]))
                stack.enter_context(patch.object(reference.platform, "machine", return_value=key[1]))
                request = stack.enter_context(patch.object(subprocess, "run", return_value=
                    subprocess.CompletedProcess([], 1, b"", b"test-only missing artifact")))
                owned = self.root / approved["runner"]
                with self.assertRaisesRegex(ValueError, "matching reviewed"):
                    reference.fetch_reviewed_reference(owned, "unknown")
                request.assert_not_called()
                self.assertFalse(owned.exists())
                with self.assertRaises(subprocess.CalledProcessError):
                    reference.fetch_reviewed_reference(owned, approved["runner"])
                request.assert_called_once_with(
                    ["gh", "api", "--hostname", "github.com", "--method", "GET",
                     f"repos/{reference.REPOSITORY}/actions/runs/{approved['run']}"],
                    capture_output=True, timeout=120, check=False)
                self.assertEqual((owned / "run.json.exit").read_text(), "1\n")
                self.assertFalse((owned / "admission.json").exists())
                self.assertFalse((owned / approved["binary"]).exists())

    def test_actual_native_entrypoint_rejects_missing_invalidated_wrong_and_pending(self):
        cases = [(None, reference.HISTORICAL), ({**self.historical, "status": "invalidated"}, reference.HISTORICAL),
                 ({**self.historical, "frozen": "c" * 40}, reference.HISTORICAL),
                 ({**self.historical, "output_sha256": "0" * 64}, reference.HISTORICAL),
                 (self.historical, reference.CORRECTED)]
        for row, selected in cases:
            with self.subTest(reference=selected, row=row):
                if row is None:
                    self.receipt.unlink()
                else:
                    self.save(row)
                with patch.object(sys, "argv", self.argv + ["--reference", selected]), \
                        patch.object(run, "invoke") as invoke, patch.object(run, "setup") as setup:
                    with self.assertRaises((ValueError, FileNotFoundError)):
                        run.main()
                invoke.assert_not_called()
                setup.assert_not_called()
                self.assertFalse(self.out.exists())

    def test_admitted_identity_reaches_real_producer_scope_only(self):
        for selected in reference.REFERENCES:
            with self.subTest(reference=selected), ExitStack() as stack:
                self.save(self.historical)
                if selected == reference.CORRECTED:
                    self.synthetic_anchor(stack)
                identity = reference.admit_receipt(selected, self.receipt, self.binary)
                stack.enter_context(patch.object(sys, "argv", self.argv + ["--reference", selected]))
                stack.enter_context(patch.object(run.subprocess, "check_output", side_effect=[b"", "synthetic-head\n"]))
                stack.enter_context(patch.object(run, "source_map", return_value={"synthetic": True}))
                stack.enter_context(patch.object(run.platform, "platform", return_value="synthetic-platform"))
                def boundary(**_kwargs):
                    self.assertEqual(bind(self.root, {}), {**BINDINGS[0], **identity})
                    raise FixtureBoundary("stop before any owned fixture/product process")
                stack.enter_context(patch.object(run.tempfile, "TemporaryDirectory", side_effect=boundary))
                invoke = stack.enter_context(patch.object(run, "invoke"))
                with self.assertRaises(FixtureBoundary):
                    run.main()
                invoke.assert_not_called()
                report = json.loads(self.out.read_bytes())
                self.assertEqual(report["reference"], identity)
                self.assertEqual(report["status"], "failed")
                self.assertFalse(report["native_surface_gate_passed"])
                self.out.unlink()
            self.assertEqual(bind(self.root, {}), BINDINGS[0])

    def test_corrected_receipt_requires_independent_anchor_and_complete_inputs(self):
        with ExitStack() as stack:
            original, anchors = self.synthetic_anchor(stack)
            reference.admit_receipt(reference.CORRECTED, self.receipt, self.binary)
            mutations = ["status", "label", "frozen", "binary", "tree", "source_after", "closure", "sdk", "toolchain"]
            for mutation in mutations:
                with self.subTest(mutation=mutation):
                    row = deepcopy(original)
                    if mutation == "status": row["status"] = "invalidated"
                    elif mutation == "label": row["reference"] = reference.HISTORICAL
                    elif mutation == "frozen": row["frozen"] = reference.FROZEN
                    elif mutation == "binary": row["output_sha256"] = "0" * 64
                    elif mutation == "tree": row["source_before"][0]["blob"] = "0" * 40
                    elif mutation == "source_after": row["source_after"] = []
                    elif mutation == "closure": row.pop("compiler_inputs_before")
                    elif mutation == "sdk":
                        row["compiler_inputs_before"].pop("sdk_files")
                        row["compiler_inputs_after"].pop("sdk_files")
                    else: row["toolchain"] = "go0.0.0"
                    digest = self.save(row)
                    with self.assertRaisesRegex(ValueError, "independently reviewed"):
                        reference.admit_receipt(reference.CORRECTED, self.receipt, self.binary)
                    # Even a test-only anchor cannot bless an invalid reference.
                    anchors[(reference.sys.platform, reference.platform.machine())] = digest
                    with self.assertRaises(ValueError):
                        reference.admit_receipt(reference.CORRECTED, self.receipt, self.binary)
                    anchors[(reference.sys.platform, reference.platform.machine())] = self.save(original)
            anchors.clear()
            with self.assertRaisesRegex(ValueError, "independently anchored"):
                reference.admit_receipt(reference.CORRECTED, self.receipt, self.binary)

    def test_compiler_collector_reads_selected_embeds_modules_and_entire_sdk(self):
        source, modules = [self.root / name for name in ("source", "modules")]
        # The pinned Go toolchain on this worker is itself in GOMODCACHE.
        sdk = modules / "golang.org/toolchain"
        go = sdk / "bin/go"
        selected = [source / "go.mod", source / "go.sum", source / "cmd/main.go",
                    source / "assets/data.txt", modules / "dep/go.mod", modules / "dep/lib.go",
                    sdk / "src/runtime.go"]
        for path in [*selected, go, sdk / "pkg/tool/link", sdk / "lib/support.dat"]:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic input, never compiled")
        settings = {"GOVERSION": reference.TOOLCHAIN, "CGO_ENABLED": "0", "GOOS": "synthetic",
                    "GOHOSTOS": "synthetic", "GOARCH": "synthetic", "GOHOSTARCH": "synthetic",
                    "GOWORK": "off", "GOFLAGS": "", "GOEXPERIMENT": "", "GOROOT": str(sdk),
                    "GOMOD": str(source / "go.mod"), "GOMODCACHE": str(modules)}
        packages = [{"Dir": str(source), "GoFiles": ["cmd/main.go"], "EmbedFiles": ["assets/data.txt"]},
                    {"Dir": str(modules / "dep"), "GoFiles": ["lib.go"]},
                    {"Dir": str(sdk / "src"), "GoFiles": ["runtime.go"]}]
        module_rows = [{"Path": "synthetic", "GoMod": str(source / "go.mod"), "Main": True},
                       {"Path": "dep", "GoMod": str(modules / "dep/go.mod"), "Version": "v0.0.0"}]
        def collect():
            results = [json.dumps(settings).encode(), b"\n".join(json.dumps(p).encode() for p in packages),
                       b"\n".join(json.dumps(m).encode() for m in module_rows)]
            with patch.object(build_go, "run", side_effect=results) as command:
                result = compiler_inputs.collect(str(go), source, {"GOMODCACHE": str(modules)}, {}, command)
            self.assertEqual([call.args[0][1] for call in command.call_args_list], ["env", "list", "list"])
            return result
        before = collect()
        self.assertEqual(len(before["files"]), len(selected))
        self.assertIn("assets/data.txt", [r["path"] for r in before["files"]])
        self.assertEqual({r["path"] for r in before["sdk_files"]},
                         {"bin/go", "src/runtime.go", "pkg/tool/link", "lib/support.dat"})
        (sdk / "lib/support.dat").write_bytes(b"changed SDK support")
        self.assertNotEqual(before, collect())
        packages[0]["GoFiles"].append(str(self.binary))
        with self.assertRaisesRegex(ValueError, "escapes"):
            collect()
        packages[0]["GoFiles"].pop()
        module_rows[1]["Replace"] = {"Dir": str(self.root)}
        with self.assertRaisesRegex(ValueError, "replacement"):
            collect()

    def test_corrected_startup_never_accepts_equal_historical_or_missing_binding(self):
        with ExitStack() as stack:
            self.synthetic_anchor(stack)
            identity = reference.admit_receipt(reference.CORRECTED, self.receipt, self.binary)
            with using_reference(identity):
                for parse_error, skills in ((False, True), (True, True), (False, False)):
                    go, rust = synthetic_pair(parse_error, skills)
                    # Equal historical fields are invalid in the corrected scope.
                    with self.assertRaisesRegex(AssertionError, "binding"):
                        pair(go, rust)
                    for record in (go, rust):
                        record["startup_input"].update(identity)
                    before = deepcopy((go, rust))
                    pair(go, rust)
                    self.assertEqual((go, rust), before)
                    rust.pop("startup_input")
                    with self.assertRaisesRegex(AssertionError, "binding"):
                        pair(go, rust)
                with self.assertRaisesRegex(AssertionError, "override"):
                    bind(self.root, {"SYMBRAIN_AUDIT_ENABLED": "true"})
            with self.assertRaisesRegex(AssertionError, "binding"):
                pair(*before)


def check_reviewed_reference(receipt_path, binary_path, historical_path, routing_key=None):
    """Read real retained inputs without invoking a product or changing trust pins."""
    check = unittest.TestCase()
    host = (sys.platform, reference.platform.machine())
    key = routing_key or host
    pins = dict(reference.CORRECTED_RECEIPTS)
    digest = pins[key]
    check.assertEqual(pins[("darwin", "arm64")], "7aaacf61391f4edc0d7ad2f0084f9296d14527f500d8a3923bd569a2f585fb74")
    raw, binary, historical_raw = [path.read_bytes() for path in (receipt_path, binary_path, historical_path)]
    check.assertEqual(hashlib.sha256(raw).hexdigest(), digest)
    binary_digest = (reference.REVIEWED_ARTIFACTS[key]["binary_sha256"] if key in reference.REVIEWED_ARTIFACTS
                     else "7047ca4db34406afedc309e86e0eeb6f6d77d0eaeb50584966ec1f598b4dec86")
    check.assertEqual(hashlib.sha256(binary).hexdigest(), binary_digest)
    check.assertEqual(hashlib.sha256(historical_raw).hexdigest(), "f30db1ea864907cd6d4b262df2ab31a7011e413d80f6795b5816f663ece423a6")
    original = json.loads(raw)
    rejected = []
    # These sentinels enforce a noncompiling, no-product boundary, not a fake
    # successful execution. Positive admission calls the shared function only.
    with tempfile.TemporaryDirectory(prefix="skills-reviewed-admission-") as temporary, ExitStack() as stack:
        root = Path(temporary)
        # Explicit routing simulation on this host, NOT foreign native evidence.
        stack.enter_context(patch.object(reference.sys, "platform", key[0]))
        stack.enter_context(patch.object(reference.platform, "machine", return_value=key[1]))
        invoke = stack.enter_context(patch.object(run, "invoke", side_effect=AssertionError("product invocation forbidden")))
        setup = stack.enter_context(patch.object(run, "setup", side_effect=AssertionError("fixture setup forbidden")))
        process = stack.enter_context(patch.object(subprocess, "Popen", side_effect=AssertionError("process forbidden")))
        identity = reference.admit_receipt(reference.CORRECTED, receipt_path, binary_path)
        check.assertEqual(identity, {"frozen_go": reference.CORRECTED_COMMIT, "reference": reference.CORRECTED,
                                     "source_tree_sha256": reference.CORRECTED_TREE, "oracle_receipt_sha256": digest})

        def refuse(name, receipt, executable, diagnostic):
            with check.assertRaisesRegex((ValueError, FileNotFoundError), diagnostic):
                reference.admit_receipt(reference.CORRECTED, receipt, executable)
            output = root / (name + "-must-not-exist.json")
            argv = ["run.py", "--reference", reference.CORRECTED, "--go", str(executable),
                    "--rust", str(binary_path), "--git", str(binary_path), "--git-fixture", str(binary_path),
                    "--oracle-receipt", str(receipt), "--out", str(output)]
            with patch.object(sys, "argv", argv), check.assertRaisesRegex((ValueError, FileNotFoundError), diagnostic):
                run.main()
            check.assertFalse(output.exists())
            rejected.append(name)

        changed = root / "test-only-mutated-receipt.json"
        for name in ("digest", "source_commit", "source_tree", "source_after", "binary_claim",
                     "invalidated", "historical", "toolchain", "compiler", "sdk", "modules", "go_env"):
            row = deepcopy(original)
            if name == "source_commit": row["frozen"] = reference.FROZEN
            elif name == "source_tree": row["source_before"][0]["blob"] = "0" * 40
            elif name == "binary_claim": row["output_sha256"] = "0" * 64
            elif name == "invalidated": row["status"] = "invalidated"
            elif name == "source_after": row["source_after"] = []
            elif name == "toolchain": row["toolchain"] = "go0.0.0"
            elif name in ("compiler", "sdk", "modules", "go_env"):
                field = {"compiler": "files", "sdk": "sdk_files"}.get(name, name)
                # Equal but invalid inventories must not bypass the exact pin.
                row["compiler_inputs_before"][field] = []
                row["compiler_inputs_after"][field] = []
            payload = raw + b" " if name == "digest" else historical_raw if name == "historical" else json.dumps(row).encode()
            changed.write_bytes(payload)
            refuse(name, changed, binary_path, "independently reviewed native build")
        changed_binary = root / "test-only-mutated-binary"
        changed_binary.write_bytes(bytes([binary[0] ^ 1]) + binary[1:])
        refuse("binary_bytes", receipt_path, changed_binary, "executable does not match")
        refuse("missing_receipt", root / "absent.json", binary_path, "No such file")
        refuse("missing_binary", receipt_path, root / "absent-binary", "No such file")
        with patch.object(reference.platform, "machine", return_value="unsupported-test-only"):
            refuse("unsupported_routing_key", receipt_path, binary_path, "independently anchored native build provenance")
        invoke.assert_not_called()
        setup.assert_not_called()
        process.assert_not_called()
    optimized = []
    for option in ("-O", "-OO"):
        module = Path(__file__).resolve().parents[1] / "skills-argv-oracle/skills_reference.py"
        result = subprocess.run([sys.executable, "-B", option, str(module)],
                                capture_output=True, timeout=10, check=False)
        check.assertEqual(result.returncode, 1)
        check.assertEqual(result.stdout, b"")
        check.assertIn(b"requires Python assertions enabled", result.stderr)
        optimized.append({"option": option, "exit": result.returncode,
                          "stdout_hex": result.stdout.hex(), "stderr_hex": result.stderr.hex()})
    check.assertEqual(reference.CORRECTED_RECEIPTS, pins)
    for path, before in ((receipt_path, raw), (binary_path, binary), (historical_path, historical_raw)):
        check.assertEqual(path.read_bytes(), before)
    return {"status": "passed", "identity": identity, "admitted": 1, "rejected": rejected,
            "host": host, "routing_key": key, "routing_simulation": key != host,
            "optimized_rejections": optimized, "product_invocations": 0, "native_parity": False}


if __name__ == "__main__":
    if "--reviewed-receipt" in sys.argv:
        parser = argparse.ArgumentParser(description=check_reviewed_reference.__doc__)
        parser.add_argument("--reviewed-receipt", type=Path, required=True)
        parser.add_argument("--reviewed-binary", type=Path, required=True)
        parser.add_argument("--historical-receipt", type=Path, required=True)
        parser.add_argument("--routing-key", choices=("darwin/arm64", "linux/x86_64", "win32/AMD64"),
                            help="test-only routing simulation; never native execution")
        args = parser.parse_args()
        key = tuple(args.routing_key.split("/")) if args.routing_key else None
        print(json.dumps(check_reviewed_reference(args.reviewed_receipt, args.reviewed_binary,
                                                  args.historical_receipt, key), indent=2))
    else:
        unittest.main()
