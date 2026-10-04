"""Actual provider Python control flow with owned simulated loader/DB callbacks.

These checks do not load an SDK/extension, execute SQL or start product children.
Native loader and complete unchanged37/9 gates remain separate requirements.
"""
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
from types import SimpleNamespace
import sys
import tempfile
import unittest
from unittest.mock import patch

import provider

# The fixture simulates Linux independently of its host's loader API. Distinct
# flag bits also let its fake loader reject missing/eager/global flag mistakes.
LINUX_RTLD_GLOBAL = 0x100
LINUX_RTLD_NOW = 0x2


class Symbol:
    def __init__(self, address):
        self.address = address

    def __call__(self):
        return provider.SOURCE_ID.encode('ascii')


class AdmissionFixture:
    def __init__(self, root, *, address=101, version=provider.VERSION,
                 source_id=provider.SOURCE_ID):
        self.root = root
        self.trace = []
        self.loader_calls = []
        self.library = root / 'libsqlite3.so.0'
        self.extension = root / 'owned-test-extension.so'
        self.library.write_bytes(b'owned loader callback fixture; not an ELF')
        self.extension.write_bytes(b'owned extension callback fixture; not an ELF')
        self.source_id = source_id
        self.library_handle = SimpleNamespace(sqlite3_sourceid=Symbol(101))
        self.extension_handle = SimpleNamespace(sqlite3_sourceid=Symbol(address))
        self.module = SimpleNamespace(sqlite_version=version, connect=self.connect)
        manifest = dict(status='built', platform='linux', sqlite_version=provider.VERSION,
                        sqlite_source_id=provider.SOURCE_ID, zip_sha256=provider.ZIP_SHA256,
                        c_sha3_256=provider.C_SHA3_256,
                        library_sha256=hashlib.sha256(self.library.read_bytes()).hexdigest())
        (root / 'provider.json').write_bytes(json.dumps(manifest).encode('utf-8'))

    def load(self, path, **kwargs):
        if path == str(self.library):
            assert kwargs == {'mode': LINUX_RTLD_GLOBAL | LINUX_RTLD_NOW}, 'verified-library loader flags'
            self.loader_calls.append((path, kwargs.copy()))
            self.trace.append('verified-library-handle')
            sys.modules['_sqlite3'] = SimpleNamespace(__file__=str(self.extension))
            sys.modules['sqlite3'] = self.module
            return self.library_handle
        assert path == str(self.extension)
        assert kwargs == {}, 'extension lookup must retain its default loader mode'
        self.loader_calls.append((path, kwargs.copy()))
        self.trace.append('actual-extension-handle')
        return self.extension_handle

    def connect(self, name):
        assert name == ':memory:'
        self.trace.append('identity-database')
        return self

    def execute(self, sql):
        assert sql == 'SELECT sqlite_source_id()'
        self.trace.append('identity-SQL-callback')
        return self

    def fetchone(self):
        return (self.source_id,)

    def close(self):
        self.trace.append('identity-close')

    def run(self, entrypoint=None):
        read_bytes = Path.read_bytes

        def read(path):
            if path == Path('/proc/self/maps'):
                return b'1-2 r-xp 0 0 1 ' + os.fsencode(self.library) + b'\n'
            return read_bytes(path)

        with ExitStack() as stack:
            stack.enter_context(patch.dict(os.environ, {provider.ENVIRONMENT: str(self.root)}))
            stack.enter_context(patch.dict(sys.modules))
            sys.modules.pop('_sqlite3', None)
            stack.enter_context(patch.object(sys, 'platform', 'linux'))
            stack.enter_context(patch.object(os, 'RTLD_NOW', LINUX_RTLD_NOW, create=True))
            stack.enter_context(patch.object(provider.ctypes, 'RTLD_GLOBAL', LINUX_RTLD_GLOBAL, create=True))
            stack.enter_context(patch.object(provider, '_active', None))
            stack.enter_context(patch.object(provider, '_library', None))
            stack.enter_context(patch.object(provider.ctypes, 'CDLL', side_effect=self.load))
            stack.enter_context(patch.object(provider.ctypes, 'cast',
                                            side_effect=lambda value, _type:
                                            SimpleNamespace(value=value.address)))
            stack.enter_context(patch.object(Path, 'read_bytes', read))
            stack.enter_context(patch.dict(sys.modules, {
                'replay': SimpleNamespace(),
                'checker': SimpleNamespace(preflight=lambda: None),
            }))
            try:
                if entrypoint is None:
                    provider.activate()
                else:
                    source = Path(__file__).with_name(entrypoint + '.py')
                    # Exact entry-point module executes its admission/imports;
                    # __main__ stays false, so no product/fixture phase starts.
                    exec(compile(source.read_bytes(), str(source), 'exec'),
                         {'__name__': 'owned_admission_probe', '__file__': str(source)})
                # Simulated consumer boundaries must be reached only after admission.
                self.trace.extend(['consumer-fixture-SQL', 'consumer-product-child'])
                return provider._active.copy()
            finally:
                assert provider.ENVIRONMENT not in os.environ


class ProviderAdmission(unittest.TestCase):
    def test_all_selected_entrypoints_admit_before_any_consumer(self):
        for entrypoint in ['checker', 'replay', 'controls']:
            with self.subTest(entrypoint=entrypoint), tempfile.TemporaryDirectory() as temporary:
                fixture = AdmissionFixture(Path(temporary))
                active = fixture.run(entrypoint)
                self.assertTrue(active['extension_sourceid_address_verified'])
                self.assertLess(fixture.trace.index('identity-close'),
                                fixture.trace.index('consumer-fixture-SQL'))

    def test_all_selected_entrypoints_reject_wrong_extension_before_SQL(self):
        for entrypoint in ['checker', 'replay', 'controls']:
            with self.subTest(entrypoint=entrypoint), tempfile.TemporaryDirectory() as temporary:
                fixture = AdmissionFixture(Path(temporary), address=102)
                with self.assertRaisesRegex(AssertionError, 'extension bound another SQLite library'):
                    fixture.run(entrypoint)
                self.assertEqual(fixture.trace, ['verified-library-handle', 'actual-extension-handle'])

    def test_healthy_selected_process_completes_identity_before_consumer(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = AdmissionFixture(Path(temporary))
            active = fixture.run()
            self.assertTrue(active['extension_sourceid_address_verified'])
            self.assertEqual(fixture.trace, [
                'verified-library-handle', 'actual-extension-handle', 'identity-database',
                'identity-SQL-callback', 'identity-close', 'consumer-fixture-SQL',
                'consumer-product-child'])

    def test_wrong_extension_rejected_before_identity_SQL_or_consumer(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = AdmissionFixture(Path(temporary), address=102)
            with self.assertRaisesRegex(AssertionError, 'extension bound another SQLite library'):
                fixture.run()
            self.assertEqual(fixture.trace, ['verified-library-handle', 'actual-extension-handle'])

    def test_wrong_engine_version_rejected_before_consumer(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = AdmissionFixture(Path(temporary), version='3.45.1')
            with self.assertRaisesRegex(AssertionError, 'required checker provider'):
                fixture.run()
            self.assertNotIn('consumer-fixture-SQL', fixture.trace)
            self.assertNotIn('consumer-product-child', fixture.trace)

    def test_wrong_SQL_source_id_rejected_before_consumer(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = AdmissionFixture(Path(temporary), source_id='another engine')
            with self.assertRaisesRegex(AssertionError, 'required checker provider'):
                fixture.run()
            self.assertNotIn('consumer-fixture-SQL', fixture.trace)
            self.assertNotIn('consumer-product-child', fixture.trace)


if __name__ == '__main__':
    unittest.main()
