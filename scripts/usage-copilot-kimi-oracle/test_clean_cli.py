"""Guard the package set needed for accepted-parent CLI transitions."""
from pathlib import Path
import unittest
from clean_cli import package_list


class PackageListTests(unittest.TestCase):
    def test_usage_transition_cleans_exact_shared_workspace_variants(self):
        self.assertEqual(
            package_list(True),
            [
                'symbrain-activity',
                'symbrain-adapter',
                'symbrain-audit',
                'symbrain-broker',
                'symbrain-catalog',
                'symbrain-cli',
                'symbrain-core',
                'symbrain-gateway',
                'symbrain-guard-core',
                'symbrain-harness',
                'symbrain-instructions',
                'symbrain-managed',
                'symbrain-mcp',
                'symbrain-memory',
                'symbrain-patterns',
                'symbrain-policy',
                'symbrain-skills',
                'symbrain-usage',
            ],
        )
        self.assertEqual(package_list(False), ['symbrain-cli'])
        self.assertNotIn('cargo clean', Path(__file__).with_name('run.sh').read_text())


if __name__ == '__main__':
    unittest.main()
