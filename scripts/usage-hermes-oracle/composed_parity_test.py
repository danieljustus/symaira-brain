"""Both documented oracle divergences must compose without hiding others."""
import base64
import copy
import json
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'usage-provider-files-oracle'))
from reference_parity import normalize_reference_resolution
from hermes_compare import hermes_login_divergence


def observation(value):
    if not isinstance(value, bytes):
        value = (json.dumps(value, ensure_ascii=False, separators=(',', ':')) + '\n').encode()
    return {'exit': 0, 'stderr_b64': '', 'stdout_b64': base64.b64encode(value).decode()}


class ComposedParityTests(unittest.TestCase):
    def test_json_composition_rejects_unrelated_changes(self):
        detail = 'credential resolution failed: synthetic env://ABSENT'
        old = 'No Nous Portal credentials found — sign in with the Hermes CLI'
        new = 'Hermes Nous login missing/expired — sign in again with the Hermes CLI'
        go = {'providers': [
            {'id': 'claude', 'configured': False, 'auth_status': {'detail': detail}},
            {'id': 'nous', 'configured': False, 'auth_status': {'status': 'missing', 'detail': old}},
        ]}
        native = copy.deepcopy(go)
        native['providers'][0].update(configured=True, error=detail)
        native['providers'][1]['auth_status'] = {'status': 'expired', 'detail': new, 'source': 'file'}
        def matches(actual, file_present=True):
            normalized = normalize_reference_resolution(observation(actual))
            return normalized is not None and hermes_login_divergence(observation(go), normalized, 'json', file_present)
        self.assertTrue(matches(native))
        self.assertFalse(matches(native, False))
        altered = copy.deepcopy(native)
        altered['providers'][0]['error'] = 'different'
        self.assertFalse(matches(altered))
        altered = copy.deepcopy(native)
        altered['providers'][1]['configured'] = True
        self.assertFalse(matches(altered))
        normalized = normalize_reference_resolution(observation(native))
        normalized['exit'] = 42
        self.assertFalse(hermes_login_divergence(observation(go), normalized, 'json', True))

    def test_table_composition_rejects_unrelated_bytes(self):
        go = ('Claude\tnot configured\tcredential resolution failed: synthetic\n'
              'Nous\tNo Nous Portal credentials found — sign in with the Hermes CLI\n').encode()
        native = go.replace(b'not configured', b'error').replace(
            'No Nous Portal credentials found — sign in with the Hermes CLI'.encode(),
            'Hermes Nous login missing/expired — sign in again with the Hermes CLI'.encode())
        normalized = normalize_reference_resolution(observation(native))
        self.assertTrue(hermes_login_divergence(observation(go), normalized, 'table', True))
        altered = normalize_reference_resolution(observation(native + b'extra'))
        self.assertFalse(hermes_login_divergence(observation(go), altered, 'table', True))


if __name__ == '__main__':
    unittest.main()
