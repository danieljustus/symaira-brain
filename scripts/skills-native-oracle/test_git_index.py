"""Actual Git bytes with synthetic MCP envelopes: comparator controls, not native parity."""
from copy import deepcopy
import hashlib
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest

from fixtures import setup
from git_index import INDEX, decode
from run import compare_pair, incoming
from test_startup_controls import add_file, synthetic_pair


def seal(raw):
    return bytes(raw[:-20]) + hashlib.sha1(raw[:-20]).digest()


def attach(record, raw):
    add_file(record, INDEX, raw, 0o644)
    record['filesystem']['raw_git_indexes'] = {INDEX: raw.hex()}


class GitIndexTest(unittest.TestCase):
    def test_optimized_interpreters_fail_before_acceptance(self):
        directory = Path(__file__).resolve().parent
        command = [sys.executable, '-B', str(directory / 'run.py'), '--help']
        env = dict(os.environ)
        env.pop('PYTHONOPTIMIZE', None)
        baseline = subprocess.run(command, env=env, capture_output=True, timeout=20)
        self.assertEqual(baseline.returncode, 0, baseline.stderr)
        for flags, override in ((['-O'], {}), (['-OO'], {}), ([], {'PYTHONOPTIMIZE': '1'})):
            for arguments in ([str(directory / 'run.py'), '--help'],
                              ['-c', 'import git_index']):
                with self.subTest(flags=flags, override=override, arguments=arguments):
                    result = subprocess.run([sys.executable, '-B', *flags, *arguments],
                                            cwd=directory, env=env | override,
                                            capture_output=True, timeout=20)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(b'Native Skills evidence requires assertions', result.stderr)
                    self.assertEqual(result.stdout, b'')

    def test_stat_only_entrypoint_and_cumulative_rejections(self):
        git = shutil.which('git')
        self.assertIsNotNone(git)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'case'
            setup(root, git)
            raw = (root / INDEX).read_bytes()
        decode(raw)
        go, rust = synthetic_pair()
        data = incoming('skills_restore', {'name': 'demo', 'rev': 'HEAD~1', 'dry_run': False})
        for record in (go, rust):
            attach(record, raw)
            record['stdin_sha256'] = hashlib.sha256(data).hexdigest()
        pair = {'id': 'mcp-018-skills_restore', 'variant': None, 'mcp': True,
                'stdin_hex': data.hex(), 'go': go, 'rust': rust}
        self.assertTrue(compare_pair(pair, Path('.')))
        original = deepcopy((go, rust))
        # Each permitted stat word really differs; checksum must still validate.
        for word in (0, 1, 2, 3, 5):
            with self.subTest(volatile_word=word):
                candidate = deepcopy(pair)
                changed = bytearray(raw)
                offset = 12 + 4 * word
                before = struct.unpack_from('!I', changed, offset)[0]
                struct.pack_into('!I', changed, offset, (before + 1) % (1_000_000_000 if word in (1, 3) else 2**32))
                attach(candidate['rust'], seal(changed))
                self.assertTrue(compare_pair(candidate, Path('.')))
        self.assertEqual((go, rust), original)
        # Coordinated byte/hash changes must reach structural/semantic rejection,
        # rather than passing because only the easy outer checksum was tested.
        mutations = [
            ('device', 12 + 16, 'nonvolatile'), ('uid', 12 + 28, 'nonvolatile'),
            ('gid', 12 + 32, 'nonvolatile'), ('size', 12 + 36, 'nonvolatile'),
            ('mode', 12 + 27, 'mode|nonvolatile'), ('object-id', 12 + 40, 'nonvolatile'),
            ('path', 12 + 62, 'unowned Git index entries'),
            ('stage', 12 + 60, 'entries'), ('flags', 12 + 60, 'flags'),
            ('padding', 12 + 71, 'padding'), ('version', 7, 'format'),
            ('count', 11, 'entry|entries|path|mode'),
            ('tree-object', len(raw) - 21, 'nonvolatile'),
            ('extension', len(raw) - 53, 'extension'),
        ]
        for name, offset, error in mutations:
            with self.subTest(reject=name):
                self.assertTrue(compare_pair(deepcopy(pair), Path('.')))
                candidate = deepcopy(pair)
                changed = bytearray(raw)
                changed[offset] ^= 0x10 if name == 'stage' else 0x80 if name == 'flags' else 1
                attach(candidate['rust'], seal(changed))
                with self.assertRaisesRegex(AssertionError, error):
                    compare_pair(candidate, Path('.'))
        for name in ('checksum', 'missing-raw', 'false-size', 'wrong-digest', 'wrong-input',
                     'other-owner', 'other-variant', 'other-path', 'missing-index'):
            with self.subTest(reject=name):
                self.assertTrue(compare_pair(deepcopy(pair), Path('.')))
                candidate = deepcopy(pair)
                record = candidate['rust']
                if name == 'checksum':
                    attach(record, raw[:-1] + bytes([raw[-1] ^ 1]))
                elif name == 'missing-raw':
                    record['filesystem']['raw_git_indexes'] = {}
                elif name == 'false-size':
                    record['filesystem']['raw_file_hashes'][INDEX]['bytes'] = True
                elif name == 'wrong-digest':
                    record['filesystem']['raw_file_hashes'][INDEX]['sha256'] = '0' * 64
                elif name == 'wrong-input':
                    record['stdin_sha256'] = '0' * 64
                elif name == 'other-variant':
                    candidate['variant'] = 'git-failure'
                elif name == 'other-path':
                    add_file(record, 'data/symbrain/skills/other/.git/index', raw)
                elif name == 'missing-index':
                    del record['filesystem']['files'][INDEX]
                else:
                    candidate['id'] = 'mcp-017-skills_restore'
                    changed = bytearray(raw)
                    changed[12] ^= 1
                    attach(record, seal(changed))
                with self.assertRaises((AssertionError, KeyError)):
                    compare_pair(candidate, Path('.'))


if __name__ == '__main__':
    unittest.main()
