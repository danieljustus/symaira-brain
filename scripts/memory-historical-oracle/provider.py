"""Bind an owned Linux checker library before importing CPython's SQLite extension."""

import ctypes
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sys

VERSION = "3.50.4"
SOURCE_ID = "2025-07-30 19:33:53 4d8adfb30e03f9cf27f800a2c1ba3c48fb4ca1b08b0f5ed59a4d5ecbf45e20a3"
ZIP_SHA256 = "1d3049dd0f830a025a53105fc79fd2ab9431aea99e137809d064d8ee8356b032"
C_SHA3_256 = "9145255e83da6529e70121ee4d7a4c88fe83ca4511da0c9ed13d10842df36782"
ENVIRONMENT = "MEMORY_SQLITE_CHECKER_PROVIDER"
_active = None
_library = None


def verified_manifest(prefix):
    prefix = Path(prefix).resolve(strict=True)
    manifest = prefix / "provider.json"
    assert not manifest.is_symlink(), "provider manifest must be owned regular data"
    data = json.loads(manifest.read_bytes())
    assert data["status"] == "built" and data["platform"] == "linux", data
    assert data["sqlite_version"] == VERSION and data["sqlite_source_id"] == SOURCE_ID
    assert data["zip_sha256"] == ZIP_SHA256 and data["c_sha3_256"] == C_SHA3_256
    library = prefix / "libsqlite3.so.0"
    assert library.is_file() and not library.is_symlink(), "missing owned library"
    assert hashlib.sha256(library.read_bytes()).hexdigest() == data["library_sha256"]
    return prefix, library, data


def activate():
    global _active, _library
    requested = os.environ.pop(ENVIRONMENT, None)
    if requested is None:
        return
    assert _active is None, "provider must be selected once before SQLite imports"
    assert sys.platform == "linux", "Darwin embeds SQLite; Windows uses its native DLL"
    assert "_sqlite3" not in sys.modules, "refuse a mixed, already loaded checker engine"
    prefix, library, manifest = verified_manifest(requested)
    # Its verified SONAME is libsqlite3.so.0, matching the pinned Linux extension.
    # This affects only this Python process, without changing loader/Python env.
    _library = ctypes.CDLL(str(library), mode=ctypes.RTLD_GLOBAL | os.RTLD_NOW)
    _library.sqlite3_sourceid.restype = ctypes.c_char_p
    assert _library.sqlite3_sourceid().decode("ascii") == SOURCE_ID
    maps = Path("/proc/self/maps").read_bytes()
    mapped = [line.split(maxsplit=5)[-1] for line in maps.splitlines()
              if len(line.split(maxsplit=5)) == 6]
    # proc maps escapes embedded newlines, but retains ordinary path spaces.
    assert os.fsencode(library).replace(b"\n", b"\\012") in mapped, "library not mapped"
    _active = dict(kind="owned-linux-sdk", prefix=str(prefix),
                   manifest_sha256=hashlib.sha256((prefix / "provider.json").read_bytes()).hexdigest(),
                   library_sha256=manifest["library_sha256"], library_mapped=True,
                   zip_sha256=ZIP_SHA256, c_sha3_256=C_SHA3_256,
                   loader_environment_changed=False, provider_environment_consumed=True)
    # Admission belongs to this interpreter, including the controls process.
    # A verified handle alone cannot establish the extension's symbol binding.
    identity()


def identity():
    import _sqlite3
    import sqlite3
    if _active is not None:
        extension = ctypes.CDLL(_sqlite3.__file__)
        actual_address = ctypes.cast(extension.sqlite3_sourceid, ctypes.c_void_p).value
        expected_address = ctypes.cast(_library.sqlite3_sourceid, ctypes.c_void_p).value
        assert actual_address == expected_address, "extension bound another SQLite library"
        assert sqlite3.sqlite_version == VERSION, (
            "CPython did not bind the required checker provider", sqlite3.sqlite_version)
    with closing(sqlite3.connect(":memory:")) as database:
        source_id = database.execute("SELECT sqlite_source_id()").fetchone()[0]
    if _active is not None:
        assert sqlite3.sqlite_version == VERSION and source_id == SOURCE_ID, (
            "CPython did not bind the required checker provider", sqlite3.sqlite_version, source_id)
        _active["extension_sourceid_address_verified"] = True
    return dict(_active or dict(kind="native-stdlib", provider_environment_consumed=False),
                extension_path=_sqlite3.__file__,
                extension_sha256=hashlib.sha256(Path(_sqlite3.__file__).read_bytes()).hexdigest(),
                sqlite_version=sqlite3.sqlite_version, sqlite_source_id=source_id)
