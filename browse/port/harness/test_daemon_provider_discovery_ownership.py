"""Provider executable ownership checks across path aliases and escapes."""
from pathlib import Path
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
