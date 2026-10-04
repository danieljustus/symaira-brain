"""Whole-SDK predicate controls using owned fixtures, never an SDK child."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import binding
import build
import owner
import sdk


class SdkOwnership(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.go = self.root / "go"
        self.rust = self.root / "rust"
        self.runner = self.root / "runner"
        self.runner.mkdir()
        self.original_cases = binding.cases()
        for root, names in ((self.go, ["bin/go", "src/fmt/doc.go", "go.env",
                                     "pkg/include/textflag.h", "pkg/include/funcdata.h",
                                     "pkg/include/asm_amd64.h", "pkg/include/asm_ppc64x.h",
                                     "pkg/include/asm_riscv64.h"]),
                            (self.rust, ["bin/cargo", "bin/rustc", "lib/compiler.so",
                                         "lib/rustlib/components"])):
            for name in names:
                self.file(root, name)
        self.expected = {"go": sdk.inventory(self.go), "rust": sdk.inventory(self.rust)}
        (self.runner / "sdk-inputs.json").write_text(json.dumps({
            "kind": "memory760-whole-sdk-v1", "platforms": {"linux-x86_64": self.expected}}))
        self.plan = {"sdk_platforms": {"linux-x86_64": {
            "go": {"files": {"bin/go": sdk.sha(self.go / "bin/go")},
                   "source_files": {"src/fmt/doc.go": sdk.sha(self.go / "src/fmt/doc.go")}},
            "rust": {"files": {"bin/cargo": sdk.sha(self.rust / "bin/cargo"),
                               "bin/rustc": sdk.sha(self.rust / "bin/rustc")}}}}}
        for item in (patch.object(binding, "HERE", self.runner),
                     patch.object(binding, "trusted", return_value=self.plan),
                     patch.object(binding.platform, "system", return_value="Linux"),
                     patch.object(binding.platform, "machine", return_value="x86_64")):
            item.start()
            self.addCleanup(item.stop)

    def file(self, root, name, data=b"owned SDK input\n"):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return path

    def admit(self):
        return binding.sdks(self.go, self.rust)

    def test_complete_fixture_is_bound_in_snapshot(self):
        result = self.admit()
        for role in ("go", "rust"):
            self.assertEqual(result[role]["physical_inventory"], self.expected[role])

    def test_root_configuration_change_and_removal_are_rejected(self):
        path = self.go / "go.env"
        path.write_bytes(b"GOFLAGS=-overlay=foreign.json\n")
        with self.assertRaisesRegex(ValueError, "unbound SDK bytes/mode: go:go.env"):
            self.admit()
        path.unlink()
        with self.assertRaisesRegex(ValueError, "missing=.*go.env"):
            self.admit()

    def test_absent_to_present_sdk_configuration_is_rejected(self):
        self.file(self.go, "new-config.env", b"GOFLAGS=-toolexec=foreign\n")
        with self.assertRaisesRegex(ValueError, "extra=.*new-config.env"):
            self.admit()

    def test_every_assembly_header_change_is_rejected(self):
        for path in sorted((self.go / "pkg/include").iterdir()):
            original = path.read_bytes()
            path.write_bytes(b"altered assembly definitions\n")
            with self.subTest(header=path.name), self.assertRaisesRegex(ValueError, "pkg/include/"):
                self.admit()
            path.write_bytes(original)
        self.admit()

    def test_extra_selected_sources_and_embedded_assets_are_rejected(self):
        for name in ("owned_added.go", "owned.s", "owned.S", "owned.h", "owned.c", "owned.syso", "embedded.txt"):
            path = self.file(self.go, "src/fmt/" + name)
            with self.subTest(input=name), self.assertRaisesRegex(ValueError, "extra=.*" + name.replace(".", r"\.")):
                self.admit()
            path.unlink()

    def test_extra_sdk_compiler_and_directory_are_rejected(self):
        path = self.file(self.rust, "bin/unbound-wrapper")
        with self.assertRaisesRegex(ValueError, "extra=.*unbound-wrapper"):
            self.admit()
        path.unlink()
        (self.go / "src/fmt/new-package").mkdir()
        with self.assertRaisesRegex(ValueError, "directories:extra=.*new-package"):
            self.admit()

    def test_runtime_and_distribution_configuration_changes_are_rejected(self):
        for name in ("lib/compiler.so", "lib/rustlib/components"):
            path = self.rust / name
            original = path.read_bytes()
            path.write_bytes(b"foreign distribution input\n")
            with self.subTest(input=name), self.assertRaisesRegex(ValueError, "unbound SDK bytes/mode: rust:"):
                self.admit()
            path.write_bytes(original)

    def test_executable_and_directory_modes_are_bound(self):
        for path in (self.go / "bin/go", self.go / "pkg/include"):
            original = path.stat().st_mode & 0o7777
            path.chmod(original ^ 0o020)
            with self.subTest(input=str(path)), self.assertRaisesRegex(ValueError, "unbound SDK bytes/mode"):
                self.admit()
            path.chmod(original)

    @unittest.skipUnless(hasattr(os, "symlink"), "platform lacks symlink API")
    def test_sdk_file_and_directory_aliases_are_rejected(self):
        for name, target in (("src/fmt/extra", "doc.go"), ("src/aliased-package", "fmt")):
            path = self.go / name
            path.symlink_to(target)
            with self.subTest(input=name), self.assertRaisesRegex(ValueError, "SDK symlink/special"):
                self.admit()
            path.unlink()

    def test_missing_or_subset_inventory_cannot_grant_admission(self):
        path = self.runner / "sdk-inputs.json"
        path.unlink()
        with self.assertRaises(FileNotFoundError):
            self.admit()
        for kind in ("invented", "memory760-whole-sdk-v1"):
            value = {"kind": kind, "platforms": {"linux-x86_64": {
                "go": {"files": {}, "directories": {}}, "rust": self.expected["rust"]}}}
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                self.admit()

    def test_empty_flags_fallback_is_excluded_and_root_is_explicit(self):
        with patch.dict(os.environ, {"GOFLAGS": "-toolexec=foreign", "GOROOT": "/foreign"}):
            env = owner.environment(self.root, self.go, self.rust, self.root / "target")
        self.assertEqual(env["GOFLAGS"], "-mod=readonly")
        self.assertEqual(env["GOROOT"], str(self.go))
        self.assertEqual(env["GOENV"], "off")

    def test_unbound_sdk_prevents_every_builder_child(self):
        self.file(self.go, "src/fmt/extra.go")
        arguments = ["build.py"]
        roots = {"go-sdk": self.go, "rust-sdk": self.rust}
        for name in ("go-source", "rust-source", "go-sdk", "rust-sdk", "go-modcache", "cargo-cache", "target"):
            path = roots.get(name, self.root / name)
            path.mkdir(exist_ok=True)
            arguments += ["--" + name, str(path)]
        arguments += ["--output", str(self.root / "output")]
        with patch("sys.argv", arguments), patch.object(binding, "sources", return_value={}), patch.object(binding, "cases", return_value=self.original_cases), patch.object(build.subprocess, "run") as child:
            with self.assertRaisesRegex(ValueError, "extra=.*extra.go"):
                build.main()
            child.assert_not_called()
        receipt = json.loads((self.root / "output/build-receipt.json").read_bytes())
        self.assertEqual(receipt["steps"], [])
        self.assertIn("unbound SDK inventory", receipt["admission_error"])

    def test_directory_enumeration_failure_is_not_a_partial_binding(self):
        def denied(*args, **kwargs):
            kwargs["onerror"](PermissionError("owned SDK directory unreadable"))
        with patch.object(sdk.os, "walk", side_effect=denied):
            with self.assertRaisesRegex(PermissionError, "owned SDK directory unreadable"):
                self.admit()


if __name__ == "__main__":
    unittest.main()
