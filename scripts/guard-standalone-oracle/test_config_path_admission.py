"""Fixture machinery only: mocks are not native Darwin/Guard acceptance."""
import errno
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import kernel_admission as admission


class KernelAdmission(unittest.TestCase):
    def test_owned_representable_unicode_is_mandatory_and_clean(self):
        with tempfile.TemporaryDirectory() as root:
            row = admission.probe('owned-\ufffd\u2028\u2029'.encode(), root)
            self.assertTrue(row['admitted'])
            self.assertTrue(row['owned_root_removed'])
            self.assertEqual(row['entries_after_cleanup'], [])
            self.assertEqual(row['product_children_started'], 0)
            self.assertEqual(list(Path(root).iterdir()), [])

    def test_only_invalid_utf8_darwin_exact_92_is_unavailable(self):
        error = OSError(92, 'Invalid or incomplete multibyte or wide character')
        self.assertEqual(admission.classify(b'owned-\xe2\x82', error, 'darwin'),
                         'unavailable-darwin-EILSEQ92')
        for component, system, number in [(b'owned-\xe2\x82', 'linux', 92),
                                          (b'owned-\xe2\x82', 'darwin', errno.EACCES),
                                          ('owned-\ufffd'.encode(), 'darwin', 92),
                                          (b'ordinary', 'darwin', 92)]:
            with self.subTest(component=component, system=system, errno=number):
                with self.assertRaises(OSError):
                    admission.classify(component, OSError(number, 'owned failure'), system)

    @unittest.skipIf(os.name == 'nt', 'invalid raw Unix filenames have no native UTF-16 fixture')
    def test_mock_darwin_probe_preserves_raw_exception_cleanup_and_no_child(self):
        actual_mkdir = Path.mkdir
        actual_exists = Path.exists
        def mkdir(path, *args, **kwargs):
            if os.fsencode(path.name) == b'owned-\xe2\x82':
                raise OSError(92, 'owned mock EILSEQ', os.fsencode(path))
            return actual_mkdir(path, *args, **kwargs)
        def exists(path, *args, **kwargs):
            if os.fsencode(path.name) == b'owned-\xe2\x82':
                raise AssertionError('unavailable filename must not be lstat-ed during cleanup')
            return actual_exists(path, *args, **kwargs)
        with tempfile.TemporaryDirectory() as root:
            with patch('kernel_admission.sys.platform', 'darwin'), patch.object(Path, 'mkdir', mkdir), \
                 patch.object(Path, 'exists', exists):
                row = admission.probe(b'owned-\xe2\x82', root)
            self.assertFalse(row['admitted'])
            self.assertEqual(row['errno'], 92)
            self.assertTrue(row['filename_bytes_hex'].endswith(b'owned-\xe2\x82'.hex()))
            self.assertTrue(row['owned_root_removed'])
            self.assertEqual(row['entries_after_cleanup'], [])
            self.assertEqual(list(Path(root).iterdir()), [])
            self.assertIn('owned mock EILSEQ', row['exception_repr'])
            self.assertEqual(row['product_children_started'], 0)

    def proof(self):
        return dict(system='darwin', errno=92, invalid_utf8=True, admitted=False,
                    disposition='unavailable-darwin-EILSEQ92', owned_root_removed=True,
                    entries_after_cleanup=[], component_hex=b'owned-\xe2\x82'.hex())

    def test_exhaustive_ledger_separates_unexecuted_from_platform_complete(self):
        excluded = [admission.unavailable('raw', self.proof())]
        controls = [admission.unavailable('lossy-human', self.proof()),
                    admission.unavailable('lossy-json', self.proof())]
        complete = [dict(id='unicode', disposition='matched')]
        row = admission.accounting(['unicode', 'raw'], complete, excluded,
                                   ['lossy-human', 'lossy-json'], [], controls)
        self.assertFalse(row['original_requested_domain_complete'])
        self.assertTrue(row['platform_admitted_complete'])
        self.assertEqual(row['executed_control_ids'], [])
        self.assertEqual(row['unavailable_control_ids'], ['lossy-human', 'lossy-json'])
        with self.assertRaisesRegex(AssertionError, 'unrecorded exclusion'):
            admission.accounting(['unicode', 'raw'], complete, [], [], [], [])
        with self.assertRaisesRegex(AssertionError, 'duplicate or overlapping'):
            admission.accounting(['raw'], [dict(id='raw')], excluded, [], [], [])
        mutant = admission.accounting_control(['unicode', 'raw'], complete, excluded,
                                              ['lossy-human', 'lossy-json'], [], controls)
        self.assertTrue(mutant['rejected'])
        self.assertFalse(mutant['product_control'])

    def test_linux_original_domain_and_failed_pairs_cannot_claim_complete(self):
        good = [dict(id='raw', disposition='matched')]
        controls = [dict(id='lossy-human', rejected=True), dict(id='lossy-json', rejected=True)]
        row = admission.accounting(['raw'], good, [], ['lossy-human', 'lossy-json'], controls, [])
        self.assertTrue(row['original_requested_domain_complete'])
        self.assertTrue(row['platform_admitted_complete'])
        failed = [dict(id='raw', disposition='failed: original comparator')]
        self.assertFalse(admission.accounting(['raw'], failed, [], [], [], [])['platform_admitted_complete'])
        false_proof = self.proof(); false_proof['system'] = 'linux'
        with self.assertRaises(AssertionError):
            admission.accounting(['raw'], [], [admission.unavailable('raw', false_proof)], [], [], [])
        with self.assertRaisesRegex(AssertionError, 'admitted control was not rejected'):
            admission.accounting(['raw'], good, [], ['lossy-human'], [dict(id='lossy-human', rejected=False)], [])


if __name__ == '__main__':
    unittest.main()
