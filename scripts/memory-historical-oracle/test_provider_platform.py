"""Prepared portable loader-API controls; no C/SQL/SDK/product is executed.

The Linux fixture supplies both loader constants even when its host has neither.
Genuine native binding, Windows CI and the original37/9 gates remain mandatory.
"""
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import provider
import test_provider_admission as admission
from test_provider_admission import (
    AdmissionFixture, LINUX_RTLD_GLOBAL, LINUX_RTLD_NOW,
)


class ProviderPortableSimulation(unittest.TestCase):
    def test_absent_host_loader_constants_run_unchanged_six_admission_methods(self):
        original_now = ('RTLD_NOW' in os.__dict__, os.__dict__.get('RTLD_NOW'))
        original_global = ('RTLD_GLOBAL' in provider.ctypes.__dict__,
                           provider.ctypes.__dict__.get('RTLD_GLOBAL'))
        with patch.dict(os.__dict__), patch.dict(provider.ctypes.__dict__):
            os.__dict__.pop('RTLD_NOW', None)
            provider.ctypes.__dict__.pop('RTLD_GLOBAL', None)
            result = unittest.TestResult()
            unittest.defaultTestLoader.loadTestsFromTestCase(admission.ProviderAdmission).run(result)
            self.assertEqual(result.testsRun, 6)
            self.assertTrue(result.wasSuccessful(), result.errors + result.failures)
            self.assertNotIn('RTLD_NOW', os.__dict__)
            self.assertNotIn('RTLD_GLOBAL', provider.ctypes.__dict__)
        self.assertEqual(('RTLD_NOW' in os.__dict__, os.__dict__.get('RTLD_NOW')), original_now)
        self.assertEqual(('RTLD_GLOBAL' in provider.ctypes.__dict__,
                          provider.ctypes.__dict__.get('RTLD_GLOBAL')), original_global)

    def test_verified_library_flags_and_default_extension_lookup_are_distinct(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = AdmissionFixture(Path(temporary))
            fixture.run()
            self.assertEqual(fixture.loader_calls, [
                (str(fixture.library), {'mode': LINUX_RTLD_GLOBAL | LINUX_RTLD_NOW}),
                (str(fixture.extension), {}),
            ])
            for mode in [LINUX_RTLD_GLOBAL, LINUX_RTLD_NOW, 0]:
                with self.subTest(mode=mode), self.assertRaisesRegex(AssertionError, 'verified-library loader flags'):
                    fixture.load(str(fixture.library), mode=mode)
            with self.assertRaisesRegex(AssertionError, 'default loader mode'):
                fixture.load(str(fixture.extension), mode=LINUX_RTLD_GLOBAL | LINUX_RTLD_NOW)
            self.assertEqual(len(fixture.loader_calls), 2)

    def test_production_windows_guard_rejects_before_simulated_loader(self):
        with patch.dict(os.environ, {provider.ENVIRONMENT: 'owned-unused-prefix'}), \
                patch.object(sys, 'platform', 'win32'), \
                patch.object(provider, '_active', None), \
                patch.object(provider.ctypes, 'CDLL') as loader:
            with self.assertRaisesRegex(AssertionError, 'Darwin embeds SQLite; Windows uses its native DLL'):
                provider.activate()
            loader.assert_not_called()
            self.assertNotIn(provider.ENVIRONMENT, os.environ)


if __name__ == '__main__':
    unittest.main()
