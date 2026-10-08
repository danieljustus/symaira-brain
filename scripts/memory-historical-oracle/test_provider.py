"""Portable source/provider failure tests; no native SDK or product compilation."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import prepare_provider
import provider

ROOT = Path(__file__).resolve().parents[2]
SDK = ROOT / "migration/evidence/memory-historical-649/sqlite-provider-f428/sqlite-amalgamation-3500400.zip"


def fixture(prefix):
    library = prefix / "libsqlite3.so.0"
    library.write_bytes(b"owned not-an-ELF test fixture")
    data = dict(status="built", platform="linux", sqlite_version=provider.VERSION,
                sqlite_source_id=provider.SOURCE_ID, zip_sha256=provider.ZIP_SHA256,
                c_sha3_256=provider.C_SHA3_256,
                library_sha256=hashlib.sha256(library.read_bytes()).hexdigest())
    (prefix / "provider.json").write_text(json.dumps(data))
    return data


class ProviderPreparation(unittest.TestCase):
    def test_full_official_archive_and_source_id(self):
        sources = prepare_provider.verified_sources(SDK)
        self.assertEqual(set(sources), {"sqlite3.c", "sqlite3.h", "sqlite3ext.h", "shell.c"})
        self.assertIn(b"author disclaims copyright", sources["sqlite3.c"][:2000])
        self.assertEqual(hashlib.sha3_256(sources["sqlite3.c"]).hexdigest(), provider.C_SHA3_256)

    def test_corrupt_sdk_rejected_before_compiler_or_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            changed = bytearray(SDK.read_bytes())
            changed[len(changed) // 2] ^= 1
            archive = root / "changed.zip"
            archive.write_bytes(changed)
            with patch.object(sys, "platform", "linux"), patch.object(subprocess, "run") as child:
                with self.assertRaisesRegex(AssertionError, "ZIP mismatch"):
                    prepare_provider.build(archive, root / "output")
                child.assert_not_called()
                self.assertFalse((root / "output").exists())

    def test_existing_prefix_never_reused(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(sys, "platform", "linux"), patch.object(subprocess, "run") as child:
                with self.assertRaises(FileExistsError):
                    prepare_provider.build(SDK, Path(temporary))
                child.assert_not_called()

    def test_compiler_failure_keeps_exact_raw_streams_and_no_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "output"
            failed = subprocess.CompletedProcess(["cc", "--version"], 73, b"raw\xff", b"failure\x00")
            with patch.object(sys, "platform", "linux"), patch.object(subprocess, "run", return_value=failed):
                with self.assertRaises(AssertionError):
                    prepare_provider.build(SDK, output)
            receipt = json.loads((output / "provider.json").read_bytes())
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["commands"][0]["exit"], 73)
            self.assertEqual(receipt["commands"][0]["stdout_hex"], "726177ff")
            self.assertEqual(receipt["commands"][0]["stderr_hex"], "6661696c75726500")

    def test_wrong_source_identity_cannot_load_library(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            data = fixture(root)
            data["sqlite_source_id"] = "unverified SDK"
            (root / "provider.json").write_text(json.dumps(data))
            with self.assertRaises(AssertionError):
                provider.verified_manifest(root)

    def test_corrupt_library_rejected_without_loader_or_environment_leak(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture(root)
            (root / "libsqlite3.so.0").write_bytes(b"changed")
            before = dict(os.environ)
            before.pop(provider.ENVIRONMENT, None)
            with patch.dict(os.environ, {provider.ENVIRONMENT: str(root)}), \
                    patch.object(sys, "platform", "linux"), patch.dict(sys.modules), \
                    patch.object(provider.ctypes, "CDLL") as loader:
                sys.modules.pop("_sqlite3", None)
                with self.assertRaises(AssertionError):
                    provider.activate()
                loader.assert_not_called()
                self.assertNotIn(provider.ENVIRONMENT, os.environ)
                self.assertEqual(dict(os.environ), before)

    def test_already_loaded_engine_is_not_mixed(self):
        with patch.dict(os.environ, {provider.ENVIRONMENT: "unused"}), \
                patch.object(sys, "platform", "linux"), patch.dict(sys.modules, {"_sqlite3": object()}), \
                patch.object(provider.ctypes, "CDLL") as loader:
            with self.assertRaisesRegex(AssertionError, "already loaded"):
                provider.activate()
            loader.assert_not_called()

    def test_static_darwin_engine_does_not_admit_elf_provider(self):
        with patch.dict(os.environ, {provider.ENVIRONMENT: "unused"}), \
                patch.object(sys, "platform", "darwin"), patch.object(provider.ctypes, "CDLL") as loader:
            with self.assertRaisesRegex(AssertionError, "Darwin embeds SQLite"):
                provider.activate()
            loader.assert_not_called()


if __name__ == "__main__":
    unittest.main()
