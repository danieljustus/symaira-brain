"""Source-only negative byte/mode/coverage controls; no product or SDK."""
import copy
import unittest
from integration_owners import check_coverage, check_identity, digest, owner_hash


class OwnerBindingTests(unittest.TestCase):
    def test_exact_original_and_current_bytes_and_modes(self):
        expected = dict(sha256=digest(b'whole published owner\n'), mode='100644')
        check_identity('owner', b'whole published owner\n', '100644', expected)
        for body, mode in [(b'altered owner\n', '100644'), (b'whole published owner\n', '100755')]:
            with self.assertRaises(AssertionError):
                check_identity('owner', body, mode, expected)

    def test_missing_or_foreign_effective_owner_rejects(self):
        check_coverage({'original', 'main', 'union'}, {'original', 'main', 'union'})
        for actual in [{'original', 'main'}, {'original', 'main', 'union', 'foreign'}]:
            with self.assertRaises(AssertionError):
                check_coverage({'original', 'main', 'union'}, actual)

    def test_historical_expectation_cannot_be_rewritten(self):
        manifest = dict(changed_owners={'owner': dict(historical=dict(sha256='original'))},
                        effective_source={'owner': dict(sha256='intended')})
        self.assertEqual(owner_hash(manifest, 'owner', 'original'), 'intended')
        self.assertEqual(owner_hash(manifest, 'unchanged', 'exact-old'), 'exact-old')
        with self.assertRaises(AssertionError):
            owner_hash(manifest, 'owner', 'forged-old')
        missing = copy.deepcopy(manifest)
        missing['changed_owners']['owner']['historical'] = None
        with self.assertRaises(AssertionError):
            owner_hash(missing, 'owner', 'original')


if __name__ == '__main__':
    unittest.main()
