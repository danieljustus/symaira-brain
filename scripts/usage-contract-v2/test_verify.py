#!/usr/bin/env python3
"""Negative controls on the real current approval path; no Git identity stubs."""
import copy
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from capture import digest, git, ROOT
from verify import artifact, validate


class Controls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.evidence = Path(os.environ['USAGE_V2_EVIDENCE']).resolve()
        cls.anchor = json.loads((ROOT / 'scripts/usage-contract-v2/accepted.json').read_bytes())
        validate(ROOT, cls.evidence, cls.anchor)

    def test_receipt_and_coordinated_replacement_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            target = Path(folder) / 'evidence'
            shutil.copytree(self.evidence, target)
            p = target / 'history-paired.json'
            p.write_bytes(b'{}\n')
            r = json.loads((target / 'receipt.json').read_bytes())
            r['artifacts'][p.name] = dict(bytes=p.stat().st_size, sha256=digest(p.read_bytes()))
            (target / 'receipt.json').write_text(json.dumps(r))
            with self.assertRaisesRegex(ValueError, 'receipt anchor mismatch'):
                validate(ROOT, target, self.anchor)

    def test_traversal_symlink_and_size_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder) / 'inside'
            root.mkdir()
            outside = Path(folder) / 'sentinel'
            outside.write_bytes(b'harmless')
            expected = dict(bytes=8, sha256=digest(b'harmless'))
            for name in ('../sentinel', str(outside)):
                with self.assertRaises(ValueError):
                    artifact(root, name, expected)
            (root / 'link').symlink_to(outside)
            with self.assertRaisesRegex(ValueError, 'escape'):
                artifact(root, 'link', expected)
            (root / 'good').write_bytes(b'harmless')
            self.assertEqual(artifact(root, 'good', expected), b'harmless')
            with self.assertRaisesRegex(ValueError, 'size'):
                artifact(root, 'good', dict(expected, bytes=7))

    def test_named_execution_guard(self):
        from capture import named
        good = 'test chosen ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;'
        named(good, 'chosen')
        for bad in ('', good.replace('chosen', 'other'), good.replace('1 passed', '0 passed'), good.replace('0 ignored', '1 ignored')):
            with self.assertRaises(AssertionError):
                named(bad, 'chosen')

    def test_bad_source_identity_and_source_manifest(self):
        for identity in ('HEAD', 'f' * 39, None):
            anchor = dict(self.anchor, source_commit=identity)
            with self.assertRaisesRegex(ValueError, 'invalid source commit'):
                validate(ROOT, self.evidence, anchor)
        anchor = copy.deepcopy(self.anchor)
        anchor['inputs']['internal/usage/schema.go'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'source freshness'):
            validate(ROOT, self.evidence, anchor)


if __name__ == '__main__':
    unittest.main()
