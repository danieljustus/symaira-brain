"""Fail-closed regression coverage for the pre-#930 oracle exception."""
import base64
import copy
import json
import unittest
from reference_parity import reference_resolution_divergence


def observation(value):
    if not isinstance(value, bytes):
        value = (json.dumps(value, ensure_ascii=False, separators=(',', ':')) + '\n').encode()
    return {'exit': 0, 'stderr_b64': '', 'stdout_b64': base64.b64encode(value).decode()}


class ReferenceParityTests(unittest.TestCase):
    def setUp(self):
        detail = 'credential resolution failed: synthetic env://ABSENT'
        self.go = {'schema_version': 1, 'providers': [
            {'id': 'claude', 'configured': False,
             'auth_status': {'status': 'missing', 'source': 'vault', 'detail': detail}},
            {'id': 'codex', 'configured': False}]}
        self.native = copy.deepcopy(self.go)
        self.native['providers'][0].update(configured=True, error=detail)

    def test_accepts_only_promotion(self):
        self.assertTrue(reference_resolution_divergence(observation(self.go), observation(self.native)))
        self.assertFalse(reference_resolution_divergence(observation(self.go), observation(self.go)))

    def test_rejects_other_fields_and_diagnostics(self):
        for field, value in [('id', 'wrong'), ('configured', False), ('error', 'wrong'),
                             ('auth_status', {'status': 'expired'})]:
            with self.subTest(field=field):
                native = copy.deepcopy(self.native)
                native['providers'][0][field] = value
                self.assertFalse(reference_resolution_divergence(observation(self.go), observation(native)))
        native = copy.deepcopy(self.native)
        native['providers'][1]['configured'] = True
        self.assertFalse(reference_resolution_divergence(observation(self.go), observation(native)))

    def test_rejects_exit_stderr_and_whitespace(self):
        for field, value in [('exit', 42), ('stderr_b64', 'eA==')]:
            native = observation(self.native)
            native[field] = value
            self.assertFalse(reference_resolution_divergence(observation(self.go), native))
        native = observation(json.dumps(self.native, indent=2).encode())
        self.assertFalse(reference_resolution_divergence(observation(self.go), native))

    def test_table_preserves_other_bytes(self):
        go = b'claude\tClaude\tnot configured\tcredential resolution failed: synthetic\n'
        native = go.replace(b'not configured', b'error')
        self.assertTrue(reference_resolution_divergence(observation(go), observation(native)))
        for changed in [native.replace(b'synthetic', b'wrong'), native + b'\n']:
            self.assertFalse(reference_resolution_divergence(observation(go), observation(changed)))


if __name__ == '__main__':
    unittest.main()
