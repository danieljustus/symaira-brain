"""Portable owned filesystem/pure fixture checks; no actual native process proof."""
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import config_paths
import config_warnings
import kernel_admission


PARENTS = ['.config/hermes', '.cursor', '.vscode', '.config/opencode',
           '.config/claude', 'Library/Application Support/Claude', 'claude']


class ConfigPathFixtures(unittest.TestCase):
    def test_windows_junction_api_is_separate_from_product_environment_callback(self):
        api = SimpleNamespace(name='nt', environ={'COMSPEC': 'owned-COMSPEC'})
        returned = subprocess.CompletedProcess([], 0, b'owned API mock', b'')
        with patch('config_paths.os', api), patch('config_paths.subprocess.run', return_value=returned) as run:
            config_paths.link_directory(Path('owned-link'), Path('owned-target'))
        argv, kwargs = run.call_args
        self.assertEqual(argv[0], ['owned-COMSPEC', '/c', 'mklink', '/J', 'owned-link', 'owned-target'])
        self.assertEqual(kwargs, dict(capture_output=True, timeout=5))
        self.assertNotIn('env', kwargs)

    def test_all_owned_possible_discovery_bases_have_parents_and_absent_files(self):
        with tempfile.TemporaryDirectory(prefix='guard-fixture-parents-') as directory:
            root = Path(directory)
            lexical, physical = root/'cfg', root/'physical/cfg'
            config_paths.prepare_discovery_parents(root, lexical, physical)
            for base in [root/'home', lexical, physical, root/'unused-config']:
                for part in PARENTS:
                    self.assertTrue((base/part).is_dir(), str(base/part))
            self.assertFalse([path for path in root.rglob('*') if path.is_file()])

    @unittest.skipIf(os.name == 'nt', 'raw Unix filenames remain a separate platform contract')
    def test_seeded_parents_preserve_raw_unix_components(self):
        with tempfile.TemporaryDirectory(prefix='guard-fixture-raw-') as directory:
            root = Path(directory)
            admission = kernel_admission.probe(b'owned-\xe2\x82<&>', root)
            if not admission['admitted']:
                self.assertEqual(admission['disposition'], 'unavailable-darwin-EILSEQ92')
                self.assertTrue(admission['owned_root_removed'])
                self.assertEqual(admission['entries_after_cleanup'], [])
                return
            leaf = os.fsdecode(b'owned-\xe2\x82<&>')
            lexical, physical = root/leaf, root/'physical'/leaf
            config_paths.prepare_discovery_parents(root, lexical, physical)
            self.assertTrue((lexical/'.config/hermes').is_dir())
            self.assertTrue((physical/'claude').is_dir())
            self.assertEqual(os.fsencode(lexical.name), b'owned-\xe2\x82<&>')

    def test_observer_adds_bad_cursor_only_after_healthy_discovery_parents(self):
        with tempfile.TemporaryDirectory(prefix='guard-fixture-observer-') as directory:
            root = Path(directory)/'case'
            case = dict(id='owned-discovery', family='home', spelling='plain',
                        state='discovery', contract='gated')
            calls = []

            def child(argv, **kwargs):
                calls.append(argv)
                self.assertEqual(kwargs['timeout'], 5)
                self.assertEqual(kwargs['env']['HOME'], str(root/'cfg-home'))
                for base in [root/'home', root/'cfg-home', root/'physical/cfg-home', root/'unused-config']:
                    for part in PARENTS:
                        self.assertTrue((base/part).is_dir(), str(base/part))
                for base in [root/'home', root/'cfg-home', root/'physical/cfg-home']:
                    self.assertEqual((base/'.cursor/mcp.json').read_bytes(), b'{bad')
                return subprocess.CompletedProcess(argv, 1, b'symguard doctor\n  Go:        go1.26.7\n',
                                                   b'owned callback diagnostic')

            # This unit observes healthy parent seeding, not the platform's
            # junction API. mklink has no product env; it must not reach child().
            with patch('config_paths.link_directory', side_effect=lambda link, target: link.mkdir()) as link_api, \
                 patch('config_paths.subprocess.run', side_effect=child):
                observed = config_paths.observe(Path('owned-not-executed'), case, root, False)
            link_api.assert_called_once_with(root/'link', root/'physical/anchor')
            self.assertTrue(observed['readonly'])
            self.assertEqual(len(calls), 1)

    def test_explicit_filename_owner_differs_only_for_symlink_parent_platform(self):
        for family in ['xdg', 'home']:
            for spelling in ['plain', 'dot', 'dotdot', 'relative', 'relative-parent', 'symlink-dotdot']:
                for explicit in [False, True]:
                    case = dict(family=family, spelling=spelling, explicit=explicit)
                    with self.subTest(**case):
                        self.assertEqual(config_paths.expected_owner(case, 'nt'), b'lexical_owner')
                        self.assertEqual(config_paths.expected_owner(case, 'posix'),
                                         b'physical_owner' if explicit and spelling == 'symlink-dotdot'
                                         else b'lexical_owner')

    def test_discovery_gate_rejects_an_earlier_missing_hermes_parent(self):
        case = dict(contract='gated', state='discovery')
        native = dict(exit_code=1, stdout_hex='', stderr_hex=config_warnings.UNSUPPORTED.hex())
        go = dict(exit_code=1, stdout_hex=b'error: discovery: hermes (owned): [unsupported] read failed'.hex(),
                  stderr_hex=b'config: warning: unknown key "lexical_owner"\n'.hex())
        with self.assertRaisesRegex(AssertionError, 'malformed Cursor not reached'):
            config_paths.compare(case, go, native)
        go['stdout_hex'] = b"error: discovery: cursor (owned): [unsupported] parse JSON: invalid character 'b' looking for beginning of object key string".hex()
        self.assertEqual(config_paths.compare(case, go, native), 'native-fail-closed-remaining-port')


if __name__ == '__main__':
    unittest.main()
