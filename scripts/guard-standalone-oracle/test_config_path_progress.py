"""Portable journal and Windows path-library controls; no native process claim."""
import ast
import io
import json
import ntpath
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from config_path_journal import Journal
import config_paths


ROOT = Path(__file__).resolve().parents[2]
ORIGINAL = ROOT / 'migration/evidence/guard-standalone-770/windows-config-owner-a1/original/publication-source/scripts/guard-standalone-oracle/config_paths.py.gz'


def wrapper_source(source):
    tree = ast.parse(source)
    control = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'control')
    write = next(n for n in ast.walk(control) if isinstance(n, ast.Call)
                 and isinstance(n.func, ast.Attribute) and n.func.attr == 'write_text')
    return write.args[0].func.value.value.replace('NATIVE', repr('owned-native.exe'))


def windows_realpath(requests):
    # Execute the actual local CPython Windows realpath function with an owned
    # GetFinalPathName stand-in, not a Windows API or a projected product run.
    tree = ast.parse(Path(ntpath.__file__).read_bytes())
    function = next(n for n in ast.walk(tree) if isinstance(n, ast.FunctionDef) and n.name == 'realpath')
    globals_ = dict(ntpath.__dict__)
    def final(path):
        requests.append(path)
        target = r'D:\owned\physical\anchor' if path == r'D:\owned\link' else path
        return '\\\\?\\' + target
    globals_['_getfinalpathname'] = final
    exec(compile(ast.Module(body=[function], type_ignores=[]), ntpath.__file__, 'exec'), globals_)
    return globals_['realpath']


class ProgressTests(unittest.TestCase):
    def setUp(self):
        self.owned = tempfile.TemporaryDirectory(prefix='guard-config-progress-owned-')
        self.addCleanup(self.owned.cleanup)
        self.root = Path(self.owned.name)

    def test_failed_control_keeps_raw_outputs_and_environment(self):
        journal = Journal(self.root/'report.json', {'source': 'owned'})
        journal.event('child-start', environment_bytes={'XDG_CONFIG_HOME': b'owned/link/../cfg'.hex()})
        journal.event('child-returned', exit_code=1, stdout_hex=b'\xff'.hex(), stderr_hex=b'wrong owner\r\n'.hex())
        journal.failed(AssertionError('incidental mutant failure'))
        actual = json.loads(journal.path.read_bytes())
        self.assertEqual(actual['status'], 'failed')
        self.assertEqual(actual['events'][1]['stdout_hex'], 'ff')
        self.assertEqual(bytes.fromhex(actual['events'][1]['stderr_hex']), b'wrong owner\r\n')
        self.assertEqual(actual['exception'], 'incidental mutant failure')

    def test_completed_pairs_survive_later_failure(self):
        journal = Journal(self.root/'report.json', {})
        journal.event('pair-complete', observation={'case': 'original', 'exit_code': 0})
        journal.failed(subprocess.TimeoutExpired(['owned'], 5, output=b'partial\xff'))
        actual = json.loads(journal.path.read_bytes())
        self.assertEqual(actual['events'][0]['observation']['case'], 'original')
        self.assertEqual(actual['exception_type'], 'TimeoutExpired')
        self.assertNotIn('result', actual)

    def test_replace_failure_preserves_previous_document_and_cleans_stage(self):
        journal = Journal(self.root/'report.json', {})
        before = journal.path.read_bytes()
        with patch('config_path_journal.os.replace', side_effect=OSError('owned replace failure')):
            with self.assertRaisesRegex(OSError, 'owned replace failure'):
                journal.event('new-event')
        self.assertEqual(journal.path.read_bytes(), before)
        self.assertEqual(list(self.root.iterdir()), [journal.path])

    def test_complete_retains_the_supplied_exact_result(self):
        journal = Journal(self.root/'report.json', {})
        result = {'total': 23, 'matched': 19, 'gated': 4, 'controls': [{'rejected': True}]}
        journal.complete(result)
        self.assertEqual(json.loads(journal.path.read_bytes())['result'], result)

    def test_actual_observer_retains_timeout_partial_streams_and_unchanged_budget(self):
        journal = Journal(self.root/'report.json', {})
        case = dict(id='owned-timeout', family='xdg', spelling='plain', contract='parity')
        error = subprocess.TimeoutExpired(['owned-native'], 5, output=b'partial\xff', stderr=None)
        with patch('config_paths.link_directory', side_effect=lambda link,target: link.mkdir()), \
             patch('config_paths.subprocess.run', side_effect=error) as run:
            with self.assertRaises(subprocess.TimeoutExpired):
                config_paths.observe(Path('/owned-native'), case, self.root/'fixture', True, journal)
        self.assertEqual(run.call_args.kwargs['timeout'], 5)
        events=json.loads(journal.path.read_bytes())['events']
        self.assertEqual(events[0]['phase'], 'child-start')
        self.assertEqual(events[1]['phase'], 'child-exception')
        self.assertEqual(bytes.fromhex(events[1]['stdout_hex']), b'partial\xff')
        self.assertIsNone(events[1]['stderr_hex'])

    def test_actual_observer_retains_child_before_readonly_assertion(self):
        journal = Journal(self.root/'report.json', {})
        case = dict(id='owned-readonly', family='xdg', spelling='plain', contract='parity')
        def changed(command, **kwargs):
            (self.root/'fixture/cfg/symguard/config.toml').write_bytes(b'owned changed state')
            return SimpleNamespace(returncode=0, stdout=b'child output\r\n', stderr=b'child error\xff')
        with patch('config_paths.link_directory', side_effect=lambda link,target: link.mkdir()), \
             patch('config_paths.subprocess.run', side_effect=changed):
            with self.assertRaisesRegex(AssertionError, 'doctor changed owned'):
                config_paths.observe(Path('/owned-native'), case, self.root/'fixture', True, journal)
        events=json.loads(journal.path.read_bytes())['events']
        self.assertEqual(events[1]['phase'], 'child-returned')
        self.assertEqual(bytes.fromhex(events[1]['stderr_hex']), b'child error\xff')
        self.assertNotEqual(events[0]['before'], events[1]['after'])

    def execute_wrapper(self, source, platform='nt', status=0, stderr=b'config: warning: unknown key "physical_owner"\n'):
        requests = []
        path = SimpleNamespace(realpath=windows_realpath(requests), normpath=ntpath.normpath, join=ntpath.join)
        if platform != 'nt':
            path = SimpleNamespace(realpath=lambda old: '/owned/physical/cfg')
        environment = {'XDG_CONFIG_HOME': r'D:\owned'+'/link/../cfg'}
        output, errors = io.BytesIO(), io.BytesIO()
        system = SimpleNamespace(argv=['wrapper.py', 'doctor'], stdout=SimpleNamespace(buffer=output),
                                 stderr=SimpleNamespace(buffer=errors), exit=lambda code: (_ for _ in ()).throw(SystemExit(code)))
        fake_os = SimpleNamespace(name=platform, environ=environment, path=path,
                                  getcwd=lambda: 'owned-cwd', fsencode=os.fsencode)
        reports = []
        class OwnedPath:
            def __init__(self, value): self.value = value
            def __truediv__(self, value): return OwnedPath(self.value+'/'+value)
            def with_suffix(self, value): return OwnedPath(self.value+value)
            def write_bytes(self, value): reports.append(json.loads(value))
            def read_bytes(self):
                self_test = self.value.replace('\\', '/')
                if '/physical/cfg/symguard/config.toml' not in self_test:
                    raise AssertionError('fixture did not select known physical sentinel')
                return b'physical_owner=1\n'
        def child(argv, **kw):
            return SimpleNamespace(returncode=status, stdout=b'owned response\r\n', stderr=stderr)
        with patch.dict(sys.modules, {'os':fake_os, 'sys':system,
                                    'subprocess':SimpleNamespace(run=child),
                                    'pathlib':SimpleNamespace(Path=OwnedPath)}):
            try:
                exec(compile(source, 'actual-generated-wrapper', 'exec'), {'__file__':'owned-wrapper.py'})
            finally:
                if '.child.json' in source:
                    self.assertEqual(len(reports), 1)
                if reports:
                    self.assertEqual(reports[0]['exit_code'], status)
                    self.assertEqual(bytes.fromhex(reports[0]['stderr_hex']), stderr)
                    self.assertEqual(bytes.fromhex(reports[0]['stdout_hex']), b'owned response\r\n')
                    self.assertEqual(reports[0]['cwd'], 'owned-cwd')
                    self.assertEqual(reports[0]['environment_bytes']['XDG_CONFIG_HOME'],
                                     os.fsencode(environment['XDG_CONFIG_HOME']).hex())
        return requests

    def test_windows_original_folds_link_parent_before_resolution(self):
        import gzip
        requests=[]
        realpath=windows_realpath(requests)
        actual=realpath(r'D:\owned'+'/link/../cfg')
        # CPython validates removal of the long-path prefix with a second call.
        self.assertEqual(requests, [r'D:\owned\cfg', r'D:\owned\cfg'])
        self.assertEqual(actual, r'D:\owned\cfg')
        original=wrapper_source(gzip.decompress(ORIGINAL.read_bytes()))
        with self.assertRaises(AssertionError):
            self.execute_wrapper(original, status=1, stderr=b'lexical_owner')

    def test_windows_current_resolves_link_prefix_first(self):
        source=wrapper_source((ROOT/'scripts/guard-standalone-oracle/config_paths.py').read_bytes())
        requests=[];realpath=windows_realpath(requests)
        resolved=realpath(r'D:\owned'+'/link')
        actual=ntpath.normpath(ntpath.join(resolved, '..', 'cfg'))
        self.assertEqual(requests, [r'D:\owned\link', r'D:\owned\physical\anchor'])
        self.assertEqual(actual, r'D:\owned\physical\cfg')
        with self.assertRaises(SystemExit) as ended: self.execute_wrapper(source)
        self.assertEqual(ended.exception.code, 0)

    def test_actual_wrapper_still_rejects_incidental_child_failure(self):
        source=wrapper_source((ROOT/'scripts/guard-standalone-oracle/config_paths.py').read_bytes())
        with self.assertRaises(AssertionError): self.execute_wrapper(source, status=1)
        with self.assertRaises(AssertionError): self.execute_wrapper(source, stderr=b'lexical_owner')

    def test_unix_wrapper_keeps_original_realpath_branch(self):
        source=wrapper_source((ROOT/'scripts/guard-standalone-oracle/config_paths.py').read_bytes())
        with self.assertRaises(SystemExit) as ended: self.execute_wrapper(source, platform='posix')
        self.assertEqual(ended.exception.code, 0)


if __name__ == '__main__':
    unittest.main()
