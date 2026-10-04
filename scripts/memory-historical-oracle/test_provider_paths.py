"""Owned fixture path normalization; simulated callbacks never load C or run SQL."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from test_provider_admission import AdmissionFixture, LINUX_RTLD_GLOBAL, LINUX_RTLD_NOW


class ProviderOwnedPaths(unittest.TestCase):
    def test_owned_baseline_derives_all_paths_from_canonical_root(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(strict=True)
            fixture = AdmissionFixture(Path(temporary))
            self.assertEqual(fixture.root, root)
            self.assertEqual(fixture.library, root / 'libsqlite3.so.0')
            self.assertEqual(fixture.extension, root / 'owned-test-extension.so')
            self.assertTrue((root / 'provider.json').is_file())
            active = fixture.run()
            self.assertEqual(active['prefix'], str(root))
            self.assertEqual(fixture.loader_calls, [
                (str(fixture.library), {'mode': LINUX_RTLD_GLOBAL | LINUX_RTLD_NOW}),
                (str(fixture.extension), {}),
            ])

    def test_owned_host_normalization_is_once_strict_before_deriving_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            canonical = Path(temporary).resolve(strict=True)
            requested = Path('owned-host-temp-alias')
            calls = []
            original = Path.resolve

            def resolve(path, *, strict=False):
                if path == requested:
                    calls.append(strict)
                    return canonical
                return original(path, strict=strict)

            # This is a particular host spelling, not broad loader path allowance.
            with patch.object(Path, 'resolve', resolve):
                fixture = AdmissionFixture(requested)
                self.assertEqual(calls, [True])
                self.assertEqual(fixture.root, canonical)
                active = fixture.run()
                self.assertEqual(calls, [True])
            self.assertEqual(active['prefix'], str(canonical))
            self.assertTrue(active['extension_sourceid_address_verified'])
            self.assertEqual(fixture.loader_calls[0][0], str(canonical / 'libsqlite3.so.0'))
            self.assertEqual(fixture.loader_calls[1][0], str(canonical / 'owned-test-extension.so'))

    def test_unrelated_loader_path_is_still_rejected_exactly(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = AdmissionFixture(Path(temporary))
            with self.assertRaises(AssertionError):
                fixture.load(str(fixture.root / 'other-library.so'),
                             mode=LINUX_RTLD_GLOBAL | LINUX_RTLD_NOW)
            self.assertEqual(fixture.loader_calls, [])
            self.assertEqual(fixture.trace, [])

    def test_missing_owned_root_fails_before_creating_fixture_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            missing = Path(temporary) / 'missing'
            with self.assertRaises(FileNotFoundError):
                AdmissionFixture(missing)
            self.assertFalse(missing.exists())


if __name__ == '__main__':
    unittest.main()
