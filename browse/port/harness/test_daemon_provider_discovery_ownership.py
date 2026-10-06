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


if __name__ == "__main__":
    unittest.main()
