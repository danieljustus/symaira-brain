"""Structural warning-runner controls, not native product parity evidence."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import config_warnings as runner


class WarningAdmission(unittest.TestCase):
    def test_main_accounts_for_refused_raw_case_without_launching_it(self):
        component = b'owned-\xe2\x82<&>'
        selected = [dict(id='unknown-all-order', data=b'owned=1\n', contract='parity'),
                    dict(id='raw-path-0', data=b'owned=1\n', contract='parity',
                         path_bytes=component)]
        proof = dict(admitted=False, system='darwin', errno=92, invalid_utf8=True,
                     component_hex=component.hex(),
                     disposition='unavailable-darwin-EILSEQ92', owned_root_removed=True,
                     entries_after_cleanup=[])
        mutants = [dict(id=ident, rejected=True) for ident, _ in runner.CONTROL_CASES]
        observed = []

        def observe(binary, case, root, native):
            self.assertEqual(case['id'], 'unknown-all-order')
            observed.append(case['id'])
            return dict(normalized_stderr_hex='')

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            go, native, report = [root/name for name in ('go', 'native', 'report.json')]
            go.write_bytes(b'structural-go-placeholder')
            native.write_bytes(b'structural-native-placeholder')
            with (patch.object(runner.sys, 'argv', ['runner', str(go), str(native), str(report)]),
                  patch.object(runner, 'cases', return_value=selected),
                  patch.object(runner.kernel_admission, 'probe', return_value=proof) as probe,
                  patch.object(runner, 'observe', side_effect=observe),
                  patch.object(runner, 'compare', return_value='matched'),
                  patch.object(runner, 'controls', return_value=mutants),
                  contextlib.redirect_stdout(io.StringIO())):
                runner.main()
                data = json.loads(report.read_bytes())
                self.assertEqual(probe.call_count, 1)
                self.assertEqual(len(observed), 12)  # One pair and ten reference repeats.
                self.assertEqual(data['requested_total'], 2)
                self.assertEqual(data['total'], 1)
                self.assertEqual(data['accounting']['unavailable_case_ids'], ['raw-path-0'])
                self.assertFalse(data['accounting']['original_requested_domain_complete'])
                self.assertTrue(data['accounting']['platform_admitted_complete'])
                self.assertFalse(data['unavailable_results'][0]['parity'])
                self.assertEqual(data['unavailable_results'][0]['product_children_started'], 0)
                self.assertTrue(data['accounting_control']['rejected'])
                # The runner must also reject a missing requested mutation control.
                with patch.object(runner, 'controls', return_value=mutants[:-1]):
                    with self.assertRaisesRegex(AssertionError, 'unrecorded exclusion'):
                        runner.main()


if __name__ == '__main__':
    unittest.main()
