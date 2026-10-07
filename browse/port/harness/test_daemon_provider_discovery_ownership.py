"""Provider executable ownership checks across path aliases and escapes."""
from pathlib import Path
import tempfile
import unittest

from unittest.mock import patch

import daemon_provider_discovery as discovery
from daemon_provider_discovery import owned_executable_relative


class OwnedExecutableTests(unittest.TestCase):
    def test_accepts_owned_executable_through_directory_alias(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            actual_root = base / "private" / "owned"
            actual_root.mkdir(parents=True)
            alias_parent = base / "tmp"
            try:
                alias_parent.symlink_to(actual_root.parent, target_is_directory=True)
            except (NotImplementedError, OSError) as error:
                self.skipTest(f"directory symlink unavailable: {error}")
            root = alias_parent / actual_root.name
            provider = base / "provider"
            provider.write_bytes(b"owned-provider-fixture")
            executable = actual_root / "bin" / "symvault"
            executable.parent.mkdir()
            executable.write_bytes(provider.read_bytes())

            self.assertFalse(executable.is_relative_to(root))
            self.assertEqual(owned_executable_relative(executable, root, provider), "bin/symvault")

    def test_rejects_outside_root_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root = base / "owned"
            root.mkdir()
            provider = root / "provider"
            provider.write_bytes(b"owned-provider-fixture")
            outside = base / "symvault"
            outside.write_bytes(provider.read_bytes())

            with self.assertRaises(AssertionError):
                owned_executable_relative(outside, root, provider)

    def test_rejects_symlink_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root = base / "owned"
            root.mkdir()
            provider = root / "provider"
            provider.write_bytes(b"owned-provider-fixture")
            outside = base / "symvault"
            outside.write_bytes(provider.read_bytes())
            executable = root / "bin" / "symvault"
            executable.parent.mkdir()
            executable.symlink_to(outside)

            with self.assertRaises(AssertionError):
                owned_executable_relative(executable, root, provider)

    def test_rejects_owned_file_with_wrong_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root = base / "owned"
            root.mkdir()
            provider = base / "provider"
            provider.write_bytes(b"expected-provider")
            executable = root / "symvault"
            executable.write_bytes(b"different-file")

            with self.assertRaises(AssertionError):
                owned_executable_relative(executable, root, provider)



class StopOwnedTests(unittest.TestCase):
    def test_unreachable_endpoint_is_a_teardown_fact_not_an_error(self):
        # A CLI that never published (or a daemon already gone) leaves no
        # pipe; the job drain, not the stop RPC, owns whatever remains.
        for error in (FileNotFoundError(2, "missing pipe"), ConnectionRefusedError(),
                      TimeoutError("Windows pipe remained busy before request")):
            with self.subTest(error=type(error).__name__):
                with patch.object(discovery.key.registry.harness, "request", side_effect=error) as request, \
                     patch.object(discovery, "Lease") as lease:
                    result = discovery.stop_owned(Path("owned-endpoint"), "owned", Path("owned-binary"))
                self.assertEqual(result, {"reachable": False, "error": repr(error)})
                self.assertEqual(request.call_count, 1)
                lease.assert_not_called()

    def test_other_transport_errors_still_fail(self):
        with patch.object(discovery.key.registry.harness, "request", side_effect=PermissionError("denied")):
            with self.assertRaises(PermissionError):
                discovery.stop_owned(Path("owned-endpoint"), "owned", Path("owned-binary"))



# Verbatim outputs from windows-11-arm run 37584912332, case bat/absolute.
GO_RAN = {"invoke_error": "", "lookup_error": "", "stdout_base64": "",
          "path_base64": "QzpcVXNlcnNcUlVOTkVSfjFcQXBwRGF0YVxMb2NhbFxUZW1wXGJkLXNjcmlwdC1icHNpamJ3eVxiaW5cc3ltdmF1bHQuYmF0"}
RUST_REFUSED = {"configured": False, "key_source": "", "error":
    'symvault entry "symbrowse/encryption-key": fork/exec '
    'C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\bd-script-bpsijbwy\\bin\\symvault.bat: '
    '%1 is not a valid Win32 application.'}


class ScriptDivergenceTests(unittest.TestCase):
    classify = staticmethod(discovery.classify_script_pair)

    def test_observed_batch_refusal_is_the_documented_divergence(self):
        self.assertEqual(self.classify("bat", "absolute", GO_RAN, RUST_REFUSED, True, False),
                         "accepted-divergence")
        self.assertEqual(len(discovery.ACCEPTED_SCRIPT_DIVERGENCE), 6)

    def test_difference_outside_the_allow_list_still_fails(self):
        for extension, mode in (("exe", "absolute"), ("bat", "implicit"), ("ps1", "raw-wide")):
            with self.subTest(extension=extension, mode=mode):
                rust = dict(RUST_REFUSED, error=RUST_REFUSED["error"].replace("symvault.bat", f"symvault.{extension}"))
                with self.assertRaisesRegex(AssertionError, "undocumented script divergence"):
                    self.classify(extension, mode, GO_RAN, rust, True, False)

    def test_allow_listed_case_fails_on_any_other_shape(self):
        cases = {
            "rust shell": (GO_RAN, RUST_REFUSED, True, True),
            "rust configured": (GO_RAN, dict(RUST_REFUSED, configured=True), True, False),
            "go lookup failed": (dict(GO_RAN, lookup_error="exec: not found"), RUST_REFUSED, False, False),
            "other rust error": (GO_RAN, dict(RUST_REFUSED, error="symvault entry: denied"), True, False),
            "other script": (GO_RAN, dict(RUST_REFUSED, error=RUST_REFUSED["error"].replace(".bat", ".cmd")), True, False),
        }
        for name, (go, rust, go_shell, rust_shell) in cases.items():
            with self.subTest(name):
                with self.assertRaises(AssertionError):
                    self.classify("bat", "absolute", go, rust, go_shell, rust_shell)

    def test_go_failure_still_requires_identical_rust_cause(self):
        failed = dict(GO_RAN, invoke_error="fork/exec C:\\x\\symvault.bat: %1 is not a valid Win32 application.")
        same = dict(RUST_REFUSED, error=discovery.PROVIDER_ERROR + failed["invoke_error"])
        self.assertEqual(self.classify("bat", "absolute", failed, same, False, False), "match")
        with self.assertRaises(AssertionError):
            self.classify("bat", "absolute", failed, RUST_REFUSED, False, False)


if __name__ == "__main__":
    unittest.main()
