"""Synthetic rejection controls; the positive check uses retained genuine Go evidence."""
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import verify

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = Path(os.environ['OPENROUTER_V1_EVIDENCE'])
FIXTURE = ROOT / 'rust/symbrain-usage/tests/fixtures/openrouter_go_v1.json'


class Controls(unittest.TestCase):
    def test_retained_source_and_raw_bytes(self):
        self.assertEqual(verify.validate(ROOT, EVIDENCE, FIXTURE)['case_count'], 27)

    def test_fixture_sabotage_rejected_without_rewrite(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory) / 'fixture.json'
            data = json.loads(FIXTURE.read_bytes())
            data['cases'][0]['report']['providers'][0]['snapshot']['balance'] = 'wrong'
            raw = json.dumps(data).encode()
            fixture.write_bytes(raw)
            with self.assertRaisesRegex(ValueError, 'fixture differs'):
                verify.validate(ROOT, EVIDENCE, fixture)
            self.assertEqual(fixture.read_bytes(), raw)

    def test_coordinated_capture_and_receipt_replacement_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'evidence'
            shutil.copytree(EVIDENCE, target)
            raw = (target / 'go.stdout').read_bytes() + b'\n'
            (target / 'go.stdout').write_bytes(raw)
            receipt = json.loads((target / 'receipt.json').read_bytes())
            receipt['artifacts']['go.stdout'] = verify.digest(raw)
            (target / 'receipt.json').write_text(json.dumps(receipt))
            with self.assertRaisesRegex(ValueError, 'receipt anchor mismatch'):
                verify.validate(ROOT, target, FIXTURE)

    def test_raw_capture_corruption_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'evidence'
            shutil.copytree(EVIDENCE, target)
            (target / 'go.stdout').write_bytes(b'{}')
            with self.assertRaisesRegex(ValueError, 'artifact mismatch'):
                verify.validate(ROOT, target, FIXTURE)

    def test_dirty_source_rejected_unit_stub(self):
        real_git = verify.git
        def dirty(root, *args):
            if args[0] == 'status':
                return ' M internal/usage/openrouter.go'
            return real_git(root, *args)
        with patch.object(verify, 'git', dirty):
            with self.assertRaisesRegex(ValueError, 'dirty captured checkout'):
                verify.validate(ROOT, EVIDENCE, FIXTURE)

    def test_execution_guard_rejects_vacuous_or_failed_children(self):
        good = 'test ' + verify.TEST + ' ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;'
        verify.executed(good, 0)
        for stdout, code in [('', 0), (good, 1), (good.replace(verify.TEST, 'wrong'), 0),
                             (good.replace('1 passed', '0 passed'), 0),
                             (good.replace('0 ignored', '1 ignored'), 0)]:
            with self.assertRaises(ValueError):
                verify.executed(stdout, code)


if __name__ == '__main__':
    unittest.main()
