#!/usr/bin/env python3
"""Run the source-bound FETCH-002 end-to-end comparison.

The retained same-package Go capture is independently validated and compared
with the pinned historical Go/AzureTLS oracle for diagnostics. The historical
oracle is invalidated evidence, so this command deliberately cannot establish
FETCH-002 acceptance. With supplied retained artifacts it executes only the
bounded Rust daemon path and a test-only Go sidecar; the release Go compat
artifact is provenance-checked but is not used as the loopback wire producer.
This command never builds or creates capture evidence; it consumes only
retained artifacts. CLI use requires an independently reviewed capture
digest, the retained executable, and matching Go build metadata. Producing
a hash from an unreviewed input at acceptance time is not independent
approval.

- recorded sources: the manifest's go-list-derived compiler-input closure is
  checked against the live working tree, including dependency source files.
- raw-wire re-derivation: `h2_settings` is independently re-parsed from
  `raw_settings_frame_b64` (a fixed 6-byte-record format, no HPACK needed).
  `header_names` is independently re-derived from `raw_header_block_b64`
  using a bounded, static-HPACK-table-only decoder (see
  `hpack_decode_names`) — it does not trust the generator's own decode.
- GREASE-aware JA3: `ja3`/`ja3_string` include random per-connection GREASE
  values (RFC 8701) and are NOT treated as a stable per-profile identity.
  `ja3_no_grease`/`ja3_string_no_grease` are independently recomputed for
  internal consistency, not proof of installed-browser identity or parity.
- fail-closed: any declared `request_error`/extra/missing field, any
  cardinality mismatch, or any bounds violation while re-parsing raw bytes
  is a controlled rejection (a `CaptureError`), never an uncaught
  exception propagating out of this module.
"""
from __future__ import annotations

import argparse
import base64
import functools
import hashlib
import json
import os
import re
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import NoReturn

MAX_FRAME_BYTES = 1 << 20

SIX_PROFILES = frozenset({"chrome", "firefox", "opera", "safari", "edge", "ios"})
JA3_RE = re.compile(r"[0-9a-f]{32}")
SHA256_RE = re.compile(r"[0-9a-f]{64}")
SHA1_RE = re.compile(r"[0-9a-f]{40}")

# GREASE code points, RFC 8701. A real client picks one at random per
# connection for the first cipher/extension/group entry, so raw JA3 differs
# on every capture of the *same* profile — only the GREASE-filtered
# fingerprint is a stable identity.
GREASE_VALUES = frozenset(
    {0x0A0A, 0x1A1A, 0x2A2A, 0x3A3A, 0x4A4A, 0x5A5A, 0x6A6A, 0x7A7A,
     0x8A8A, 0x9A9A, 0xAAAA, 0xBABA, 0xCACA, 0xDADA, 0xEAEA, 0xFAFA}
)

REQUIRED_PROFILE_FIELDS = frozenset({
    "profile",
    "protocol",
    "raw_client_hello_record_b64",
    "raw_settings_frame_b64",
    "raw_header_block_b64",
    "tls_client_version",
    "cipher_suites",
    "extensions",
    "supported_groups",
    "ec_point_formats",
    "ja3_string",
    "ja3",
    "cipher_suites_no_grease",
    "extensions_no_grease",
    "supported_groups_no_grease",
    "ja3_string_no_grease",
    "ja3_no_grease",
    "h2_settings",
    "header_names",
})

REQUIRED_TOP_FIELDS = frozenset({
    "schema_version",
    "capture_kind",
    "note",
    "worktree_head",
    "compiler",
    "executable_sha256",
    "executable_path",
    "build_inputs",
    "captured_at_utc",
    "profiles",
})

HISTORICAL_ORACLE_REL_PATH = "browse/docs/rust-port/rust009-tls-results.json"
HISTORICAL_ORACLE_SOURCE_COMMIT = "e86c1db46ad758d89372640473a5311525e3edf1"
HISTORICAL_ORACLE_FIXTURE_BLOB = "ebd2bec169b4c1aebc546672ae2fbcb8693a3e1c"
HISTORICAL_ORACLE_IDENTITY = "Go azuretls-client v1.13.2"
HISTORICAL_AZURETLS_MODULE = "v1.13.2"
EXPECTED_GO_MODULE = "github.com/danieljustus/symaira-browse"
ORACLE_COMPARISON_FIELDS = (
    ("protocol", "protocol"),
    ("tls_client_version", "tls_version"),
    ("cipher_suites_no_grease", "cipher_suites"),
    ("extensions_no_grease", "extensions"),
    ("h2_settings", "h2_settings"),
    ("header_names", "header_names"),
)
FETCH_PROFILES = ("chrome", "edge", "firefox", "ios", "opera", "safari")
ACCEPTANCE_BLOCKERS = (
    "historical oracle verdict is INVALIDATED; matching fields are diagnostic only",
    "raw-wire comparison is diagnostic only; it does not establish native Rust parity",
    "independently reviewed binary/source provenance is missing; self-computed digests are not independent approval",
    "the supplied release Go compat artifact was not the wire producer; the same-package test sidecar was used for process-local TLS trust",
)


class CaptureError(ValueError):
    pass


def _fail(message: str) -> NoReturn:
    raise CaptureError(message)


def _require_int_list(value: object, label: str) -> list[int]:
    if not isinstance(value, list) or not all(
        isinstance(v, int) and not isinstance(v, bool) for v in value
    ):
        _fail(f"{label} must be a list of ints")
    return value  # type: ignore[return-value]


def _require_str_list(value: object, label: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        _fail(f"{label} must be a list of strings")
    return value  # type: ignore[return-value]


def _b64decode(value: object, label: str) -> bytes:
    if not isinstance(value, str):
        _fail(f"{label} must be a string")
    try:
        return base64.b64decode(value, validate=True)
    except Exception as error:  # noqa: BLE001 - reject any decode failure honestly
        _fail(f"{label} is not valid base64: {error}")
        raise  # unreachable, keeps type-checkers happy


# ---------------------------------------------------------------------------
# Independent re-parse of the raw ClientHello bytes. Bounds-checked at every
# step (a truncated/corrupt input must raise CaptureError, never an
# uncaught IndexError).
# ---------------------------------------------------------------------------


def reparse_client_hello(raw: bytes) -> dict:
    if len(raw) < 5:
        _fail("raw_client_hello_record_b64: too short for a TLS record header")
    if raw[0] != 0x16:
        _fail(f"raw_client_hello_record_b64: not a TLS handshake record (type {raw[0]})")
    rec_len = int.from_bytes(raw[3:5], "big")
    if len(raw) < 5 + rec_len:
        _fail("raw_client_hello_record_b64: truncated TLS record")
    payload = raw[5 : 5 + rec_len]
    if len(payload) < 4:
        _fail("raw_client_hello_record_b64: truncated handshake message header")
    if payload[0] != 0x01:
        _fail(f"raw_client_hello_record_b64: not a ClientHello handshake message (type {payload[0]})")
    hs_len = int.from_bytes(payload[1:4], "big")
    if len(payload) < 4 + hs_len:
        _fail("raw_client_hello_record_b64: ClientHello body is truncated")
    body = payload[4 : 4 + hs_len]
    pos = 0
    if len(body) < pos + 2:
        _fail("raw_client_hello_record_b64: truncated client_version")
    legacy_version = int.from_bytes(body[pos : pos + 2], "big")
    pos += 2 + 32
    if len(body) < pos + 1:
        _fail("raw_client_hello_record_b64: truncated session_id length")
    sid_len = body[pos]
    pos += 1
    if len(body) < pos + sid_len:
        _fail("raw_client_hello_record_b64: truncated session_id")
    pos += sid_len
    if len(body) < pos + 2:
        _fail("raw_client_hello_record_b64: truncated cipher_suites length")
    cs_len = int.from_bytes(body[pos : pos + 2], "big")
    pos += 2
    if cs_len % 2 != 0 or len(body) < pos + cs_len:
        _fail("raw_client_hello_record_b64: truncated cipher_suites")
    ciphers = [int.from_bytes(body[pos + i : pos + i + 2], "big") for i in range(0, cs_len, 2)]
    pos += cs_len
    if len(body) < pos + 1:
        _fail("raw_client_hello_record_b64: truncated compression_methods length")
    cm_len = body[pos]
    pos += 1
    if len(body) < pos + cm_len:
        _fail("raw_client_hello_record_b64: truncated compression_methods")
    pos += cm_len

    extensions: list[int] = []
    groups: list[int] = []
    points: list[int] = []
    if pos < len(body):
        if len(body) < pos + 2:
            _fail("raw_client_hello_record_b64: truncated extensions length")
        ext_len = int.from_bytes(body[pos : pos + 2], "big")
        pos += 2
        end = pos + ext_len
        if end > len(body):
            _fail("raw_client_hello_record_b64: truncated extensions block")
        while pos < end:
            if pos + 4 > end:
                _fail("raw_client_hello_record_b64: truncated extension header")
            ext_type = int.from_bytes(body[pos : pos + 2], "big")
            ext_data_len = int.from_bytes(body[pos + 2 : pos + 4], "big")
            pos += 4
            if pos + ext_data_len > end:
                _fail(f"raw_client_hello_record_b64: truncated extension data for type {ext_type}")
            ext_data = body[pos : pos + ext_data_len]
            extensions.append(ext_type)
            if ext_type == 10 and len(ext_data) >= 2:
                glen = int.from_bytes(ext_data[0:2], "big")
                gd = ext_data[2:]
                usable = min(glen, len(gd) - (len(gd) % 2))
                groups = [int.from_bytes(gd[i : i + 2], "big") for i in range(0, usable, 2)]
            if ext_type == 11 and len(ext_data) >= 1:
                plen = ext_data[0]
                points = list(ext_data[1 : 1 + plen])
            pos += ext_data_len
    return {
        "tls_client_version": legacy_version,
        "cipher_suites": ciphers,
        "extensions": extensions,
        "supported_groups": groups,
        "ec_point_formats": points,
    }


def _filter_grease(values: list[int]) -> list[int]:
    return [v for v in values if v not in GREASE_VALUES]


def _ja3_string(version: int, ciphers: list[int], extensions: list[int], groups: list[int], points: list[int]) -> str:
    join = lambda xs: "-".join(str(x) for x in xs)  # noqa: E731
    return f"{version},{join(ciphers)},{join(extensions)},{join(groups)},{join(points)}"


# ---------------------------------------------------------------------------
# Independent re-derivation of the raw HTTP/2 SETTINGS frame (fixed 6-byte
# record format, no HPACK involved).
# ---------------------------------------------------------------------------


def reparse_settings_frame(payload: bytes) -> list[str]:
    if len(payload) % 6 != 0:
        _fail(f"raw_settings_frame_b64: length {len(payload)} is not a multiple of 6")
    out = []
    for i in range(0, len(payload), 6):
        setting_id = int.from_bytes(payload[i : i + 2], "big")
        value = int.from_bytes(payload[i + 2 : i + 6], "big")
        out.append(f"{setting_id}={value}")
    return out


# ---------------------------------------------------------------------------
# Bounded, static-table-only HPACK header NAME decoder (RFC 7541). This
# deliberately does not decode header VALUES (not needed for header_names)
# and fails closed on a literal Huffman-encoded NAME or an out-of-range
# index, rather than guessing. In every real capture observed so far, all
# six profiles' :method/:authority/:scheme/:path/accept-encoding/user-agent
# names are always resolved via a static-table index (never a literal
# name), so this bounded scope covers the real cases.
# ---------------------------------------------------------------------------

_HPACK_STATIC_TABLE = [
    (":authority", ""), (":method", "GET"), (":method", "POST"),
    (":path", "/"), (":path", "/index.html"), (":scheme", "http"),
    (":scheme", "https"), (":status", "200"), (":status", "204"),
    (":status", "206"), (":status", "304"), (":status", "400"),
    (":status", "404"), (":status", "500"), ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"), ("accept-language", ""),
    ("accept-ranges", ""), ("accept", ""), ("access-control-allow-origin", ""),
    ("age", ""), ("allow", ""), ("authorization", ""), ("cache-control", ""),
    ("content-disposition", ""), ("content-encoding", ""), ("content-language", ""),
    ("content-length", ""), ("content-location", ""), ("content-range", ""),
    ("content-type", ""), ("cookie", ""), ("date", ""), ("etag", ""),
    ("expect", ""), ("expires", ""), ("from", ""), ("host", ""),
    ("if-match", ""), ("if-modified-since", ""), ("if-none-match", ""),
    ("if-range", ""), ("if-unmodified-since", ""), ("last-modified", ""),
    ("link", ""), ("location", ""), ("max-forwards", ""),
    ("proxy-authenticate", ""), ("proxy-authorization", ""), ("range", ""),
    ("referer", ""), ("refresh", ""), ("retry-after", ""), ("server", ""),
    ("set-cookie", ""), ("strict-transport-security", ""), ("transfer-encoding", ""),
    ("user-agent", ""), ("vary", ""), ("via", ""), ("www-authenticate", ""),
]
assert len(_HPACK_STATIC_TABLE) == 61


def _hpack_read_int(data: bytes, pos: int, prefix_bits: int) -> tuple[int, int]:
    if pos >= len(data):
        _fail("hpack: truncated integer")
    max_prefix = (1 << prefix_bits) - 1
    value = data[pos] & max_prefix
    pos += 1
    if value < max_prefix:
        return value, pos
    shift = 0
    while True:
        if pos >= len(data):
            _fail("hpack: truncated integer continuation")
        b = data[pos]
        pos += 1
        value += (b & 0x7F) << shift
        shift += 7
        if not (b & 0x80):
            break
        if shift > 63:
            _fail("hpack: integer continuation too long")
    return value, pos


def _hpack_skip_string(data: bytes, pos: int) -> tuple[bytes, bool, int]:
    if pos >= len(data):
        _fail("hpack: truncated string literal")
    huffman = bool(data[pos] & 0x80)
    length, pos = _hpack_read_int(data, pos, 7)
    if pos + length > len(data):
        _fail("hpack: string literal length exceeds block")
    raw = data[pos : pos + length]
    pos += length
    return raw, huffman, pos


def hpack_decode_names(block: bytes) -> list[str]:
    dynamic: list[tuple[str, str]] = []  # index 62 == dynamic[0], newest first

    def lookup(index: int) -> tuple[str, str]:
        if 1 <= index <= 61:
            return _HPACK_STATIC_TABLE[index - 1]
        d = index - 62
        if 0 <= d < len(dynamic):
            return dynamic[d]
        _fail(f"hpack: index {index} out of range ({len(dynamic)} dynamic entries)")
        raise AssertionError("unreachable")

    def add_dynamic(name: str, value: str) -> None:
        dynamic.insert(0, (name, value))
        max_size = 4096  # RFC 7541 default; this capture server never sends
        # a SETTINGS_HEADER_TABLE_SIZE override, so the encoder must use it.
        total = sum(32 + len(n) + len(v) for n, v in dynamic)
        while total > max_size and dynamic:
            n, v = dynamic.pop()
            total -= 32 + len(n) + len(v)

    names: list[str] = []
    pos = 0
    while pos < len(block):
        b0 = block[pos]
        if b0 & 0x80:  # Indexed Header Field
            index, pos = _hpack_read_int(block, pos, 7)
            if index == 0:
                _fail("hpack: index 0 is invalid for an Indexed Header Field")
            name, _ = lookup(index)
            names.append(name)
        elif b0 & 0x40:  # Literal Header Field with Incremental Indexing
            index, pos = _hpack_read_int(block, pos, 6)
            if index == 0:
                raw, huffman, pos = _hpack_skip_string(block, pos)
                if huffman:
                    _fail("hpack: literal header NAME is Huffman-encoded (out of bounded re-derivation scope)")
                name = raw.decode("latin1")
            else:
                name, _ = lookup(index)
            _, _, pos = _hpack_skip_string(block, pos)  # value bytes: skip only, never needed for names
            names.append(name)
            add_dynamic(name, "")
        elif (b0 & 0xE0) == 0x20:  # Dynamic Table Size Update
            _, pos = _hpack_read_int(block, pos, 5)
        else:  # Literal without indexing (0000) or never indexed (0001)
            index, pos = _hpack_read_int(block, pos, 4)
            if index == 0:
                raw, huffman, pos = _hpack_skip_string(block, pos)
                if huffman:
                    _fail("hpack: literal header NAME is Huffman-encoded (out of bounded re-derivation scope)")
                name = raw.decode("latin1")
            else:
                name, _ = lookup(index)
            _, _, pos = _hpack_skip_string(block, pos)
            names.append(name)
    return names


# ---------------------------------------------------------------------------
# Profile-level validation
# ---------------------------------------------------------------------------


def _validate_profile_case(case: object) -> str:
    if not isinstance(case, dict):
        _fail("profile case must be a JSON object")
    if case.keys() != REQUIRED_PROFILE_FIELDS:
        missing = REQUIRED_PROFILE_FIELDS - case.keys()
        extra = case.keys() - REQUIRED_PROFILE_FIELDS
        _fail(f"profile case field mismatch: missing={sorted(missing)} extra={sorted(extra)}")

    profile = case["profile"]
    if not isinstance(profile, str) or profile not in SIX_PROFILES:
        _fail(f"unknown or non-string profile id: {profile!r}")
    if not isinstance(case["protocol"], str) or case["protocol"] != "h2":
        _fail(f"{profile}: protocol must be the string 'h2', got {case['protocol']!r}")

    raw_hello = _b64decode(case["raw_client_hello_record_b64"], f"{profile}: raw_client_hello_record_b64")
    reparsed = reparse_client_hello(raw_hello)
    _require_int_list(case["cipher_suites"], f"{profile}: cipher_suites")
    _require_int_list(case["extensions"], f"{profile}: extensions")
    _require_int_list(case["supported_groups"], f"{profile}: supported_groups")
    _require_int_list(case["ec_point_formats"], f"{profile}: ec_point_formats")
    for field in ("tls_client_version", "cipher_suites", "extensions", "supported_groups", "ec_point_formats"):
        if case[field] != reparsed[field]:
            _fail(
                f"{profile}: declared {field} does not match an independent re-parse of "
                f"raw_client_hello_record_b64 (declared {case[field]!r}, re-parsed {reparsed[field]!r})"
            )

    raw_ja3_string = _ja3_string(
        reparsed["tls_client_version"], reparsed["cipher_suites"], reparsed["extensions"],
        reparsed["supported_groups"], reparsed["ec_point_formats"],
    )
    if not isinstance(case["ja3_string"], str) or case["ja3_string"] != raw_ja3_string:
        _fail(f"{profile}: ja3_string is internally inconsistent with the raw ClientHello bytes")
    if not isinstance(case["ja3"], str) or not JA3_RE.fullmatch(case["ja3"]):
        _fail(f"{profile}: ja3 must be a 32-hex-char digest")
    if case["ja3"] != hashlib.md5(case["ja3_string"].encode()).hexdigest():  # noqa: S324 - JA3 is defined as MD5
        _fail(f"{profile}: ja3 does not match md5(ja3_string) — internally inconsistent summary")

    no_grease_ciphers = _filter_grease(reparsed["cipher_suites"])
    no_grease_ext = _filter_grease(reparsed["extensions"])
    no_grease_groups = _filter_grease(reparsed["supported_groups"])
    for field, expected in (
        ("cipher_suites_no_grease", no_grease_ciphers),
        ("extensions_no_grease", no_grease_ext),
        ("supported_groups_no_grease", no_grease_groups),
    ):
        _require_int_list(case[field], f"{profile}: {field}")
        if case[field] != expected:
            _fail(f"{profile}: {field} does not match GREASE-filtered raw ClientHello fields")
    no_grease_ja3_string = _ja3_string(
        reparsed["tls_client_version"], no_grease_ciphers, no_grease_ext, no_grease_groups,
        reparsed["ec_point_formats"],
    )
    if not isinstance(case["ja3_string_no_grease"], str) or case["ja3_string_no_grease"] != no_grease_ja3_string:
        _fail(f"{profile}: ja3_string_no_grease is internally inconsistent with the GREASE-filtered fields")
    if case["ja3_no_grease"] != hashlib.md5(no_grease_ja3_string.encode()).hexdigest():  # noqa: S324
        _fail(f"{profile}: ja3_no_grease does not match md5(ja3_string_no_grease)")

    raw_settings = _b64decode(case["raw_settings_frame_b64"], f"{profile}: raw_settings_frame_b64")
    reparsed_settings = reparse_settings_frame(raw_settings)
    declared_settings = _require_str_list(case["h2_settings"], f"{profile}: h2_settings")
    if declared_settings != reparsed_settings:
        _fail(
            f"{profile}: declared h2_settings does not match an independent re-parse of "
            f"raw_settings_frame_b64 (declared {declared_settings!r}, re-parsed {reparsed_settings!r})"
        )

    raw_headers = _b64decode(case["raw_header_block_b64"], f"{profile}: raw_header_block_b64")
    reparsed_names = hpack_decode_names(raw_headers)
    declared_names = _require_str_list(case["header_names"], f"{profile}: header_names")
    if not declared_names:
        _fail(f"{profile}: header_names must be non-empty")
    if declared_names != reparsed_names:
        _fail(
            f"{profile}: declared header_names does not match an independent HPACK re-derivation of "
            f"raw_header_block_b64 (declared {declared_names!r}, re-derived {reparsed_names!r})"
        )
    return profile


# ---------------------------------------------------------------------------
# Top-level / source-binding validation
# ---------------------------------------------------------------------------


def sha256_of_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git_blob_at_head(repo_root: Path, repo_rel_path: str) -> str | None:
    """Returns the blob hash if committed at HEAD, or None if not (e.g. an
    uncommitted new overlay file — sha256 is then the only available
    identity for it, which is still checked)."""
    result = subprocess.run(
        ["git", "rev-parse", f"HEAD:{repo_rel_path}"],
        cwd=repo_root, capture_output=True, text=True, check=False,
    )
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def verified_external_go_environment() -> dict[str, str]:
    """Require the task's Go caches to remain on the encrypted NVMe volume."""
    nvme_root = Path("/Volumes/1TB_NVMe_SN850X").resolve()
    environment = os.environ.copy()
    for name in ("FETCH_GO_CACHE", "FETCH_GO_MODCACHE"):
        value = environment.get(name)
        if not value:
            _fail(f"{name} is required and must name a verified NVMe cache path")
        path = Path(value).expanduser().resolve()
        if not path.is_dir() or not path.is_relative_to(nvme_root):
            _fail(f"{name} must be an existing directory under {nvme_root}")
        environment["GOCACHE" if name == "FETCH_GO_CACHE" else "GOMODCACHE"] = str(path)
    environment.update({
        "CGO_ENABLED": "0", "GOTOOLCHAIN": "local", "GOWORK": "off",
        "GOPROXY": "off", "GOFLAGS": "-mod=readonly", "GOMAXPROCS": "2",
    })
    return environment


@functools.lru_cache(maxsize=4)
def compiler_input_closure(repo_root_string: str, go: str = "go") -> frozenset[str]:
    """Return the files selected by Go for the actual capture test binary."""
    repo_root = Path(repo_root_string)
    try:
        result = subprocess.run(
            [go, "list", "-deps", "-test", "-json", "./internal/fetch/fetch"],
            cwd=repo_root / "browse", env=verified_external_go_environment(),
            capture_output=True, text=True, check=False, timeout=60,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        _fail(f"cannot resolve Go compiler input closure: {error}")
    if result.returncode:
        _fail(f"cannot resolve Go compiler input closure: {result.stderr.strip()}")
    decoder = json.JSONDecoder()
    remaining = result.stdout
    files: set[str] = set()
    fields = (
        "GoFiles", "CgoFiles", "SFiles", "SysoFiles", "CFiles", "HFiles",
        "CXXFiles", "MFiles", "FFiles", "EmbedFiles", "TestGoFiles", "XTestGoFiles",
    )
    while remaining.strip():
        remaining = remaining.lstrip()
        try:
            package, end = decoder.raw_decode(remaining)
        except json.JSONDecodeError as error:
            _fail(f"cannot decode Go compiler input closure: {error}")
        remaining = remaining[end:]
        if not isinstance(package, dict) or not isinstance(package.get("Dir"), str):
            _fail("Go compiler input closure contains a package without Dir")
        for field in fields:
            for name in package.get(field, []):
                if isinstance(name, str):
                    files.add(str((Path(package["Dir"]) / name).resolve()))
    files.update({str((repo_root / "browse/go.mod").resolve()), str((repo_root / "browse/go.sum").resolve())})
    if not files:
        _fail("Go compiler input closure is empty")
    return frozenset(files)


def resolve_manifest_path(repo_root: Path, value: str) -> tuple[Path, str | None]:
    path = Path(value)
    if path.is_absolute():
        return path, None
    resolved = (repo_root / path).resolve()
    if not resolved.is_relative_to(repo_root.resolve()):
        _fail(f"{value}: source path escapes the repository")
    return resolved, path.as_posix()


def validate_capture(capture: object, *, repo_root: Path, go: str = "go") -> None:
    """Raise CaptureError if `capture` is not a fresh, fully source-bound,
    internally consistent FETCH-002 compat-wire capture."""
    if not isinstance(capture, dict):
        _fail("capture must be a JSON object")
    if capture.keys() != REQUIRED_TOP_FIELDS:
        missing = REQUIRED_TOP_FIELDS - capture.keys()
        extra = capture.keys() - REQUIRED_TOP_FIELDS
        _fail(f"top-level field mismatch: missing={sorted(missing)} extra={sorted(extra)}")

    capture_kind = capture["capture_kind"]
    if capture_kind not in {"loopback_hermetic_production_client", "loopback_hermetic_compat_sidecar"}:
        _fail(f"unsupported capture_kind: {capture_kind!r}")
    if not isinstance(capture["note"], str) or not capture["note"]:
        _fail("note must be a non-empty string")
    captured_at = capture["captured_at_utc"]
    if not isinstance(captured_at, str) or not captured_at.endswith("Z"):
        _fail("captured_at_utc must be an RFC3339 UTC string")

    schema_version = capture["schema_version"]
    if type(schema_version) is not int or schema_version != 2:  # noqa: E721 - reject bool explicitly
        _fail(f"unsupported schema_version: {schema_version!r} (expected int 2)")

    worktree_head = capture["worktree_head"]
    if not isinstance(worktree_head, str) or not SHA1_RE.fullmatch(worktree_head):
        _fail(f"worktree_head is not a 40-hex-char sha: {worktree_head!r}")
    current_head = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=repo_root, capture_output=True, text=True, check=False
    ).stdout.strip()
    if worktree_head != current_head:
        _fail(
            "stale capture: worktree_head does not match the current repository HEAD "
            f"(capture claims {worktree_head}, HEAD is {current_head})"
        )

    compiler = capture["compiler"]
    if not isinstance(compiler, dict) or not {"go_version", "goos", "goarch"} <= compiler.keys():
        _fail("compiler must record go_version, goos and goarch")
    if any(not isinstance(compiler[field], str) or not compiler[field] for field in ("go_version", "goos", "goarch")):
        _fail("compiler go_version, goos and goarch must be non-empty strings")
    if not isinstance(capture["executable_sha256"], str) or not SHA256_RE.fullmatch(capture["executable_sha256"]):
        _fail("executable_sha256 must be a 64-hex-char digest")
    if not isinstance(capture["executable_path"], str) or not capture["executable_path"]:
        _fail("executable_path must be a non-empty string")

    build_inputs = capture["build_inputs"]
    if not isinstance(build_inputs, list):
        _fail("build_inputs must be a list")
    declared_paths = set()
    for entry in build_inputs:
        if not isinstance(entry, dict) or not isinstance(entry.get("path"), str):
            _fail("each build_inputs entry needs a string path")
        path = entry["path"]
        if path in declared_paths:
            _fail(f"build_inputs: duplicate path {path}")
        declared_paths.add(path)
        declared_sha256 = entry.get("sha256")
        if not isinstance(declared_sha256, str) or not SHA256_RE.fullmatch(declared_sha256):
            _fail(f"{path}: sha256 is not a 64-hex-char digest")
        working_tree_path, repo_rel_path = resolve_manifest_path(repo_root, path)
        if not working_tree_path.is_file():
            _fail(f"{path}: not found in the working tree at {working_tree_path}")
        actual_sha256 = sha256_of_file(working_tree_path)
        if actual_sha256 != declared_sha256:
            _fail(
                f"stale or corrupted capture: {path} sha256 mismatch "
                f"(capture declares {declared_sha256}, working tree currently has {actual_sha256})"
            )
        declared_blob = entry.get("git_blob_at_head")
        actual_blob = git_blob_at_head(repo_root, repo_rel_path) if repo_rel_path is not None else None
        if actual_blob is not None:
            if declared_blob != actual_blob:
                _fail(
                    f"stale or corrupted capture: {path} git blob mismatch "
                    f"(capture declares {declared_blob!r}, HEAD currently has {actual_blob})"
                )
        elif declared_blob:
            _fail(f"{path}: capture declares git_blob_at_head {declared_blob!r} but HEAD has no such blob now")
    actual_paths = compiler_input_closure(str(repo_root.resolve()), go)
    actual_manifest_paths = {
        str(resolve_manifest_path(repo_root, path)[0].resolve()) for path in declared_paths
    }
    if actual_manifest_paths != set(actual_paths):
        missing = set(actual_paths) - actual_manifest_paths
        extra = actual_manifest_paths - set(actual_paths)
        _fail(
            "build_inputs does not match the required closure: "
            f"missing paths={sorted(missing)} unexpected paths={sorted(extra)}"
        )

    profiles = capture["profiles"]
    if not isinstance(profiles, list):
        _fail("capture.profiles must be a list")
    seen = [_validate_profile_case(case) for case in profiles]
    if len(seen) != len(SIX_PROFILES) or set(seen) != SIX_PROFILES or len(seen) != len(set(seen)):
        _fail(
            f"capture must contain exactly the six profiles {sorted(SIX_PROFILES)} once each, "
            f"got {sorted(seen)}"
        )


def wait_for_path(path: Path, timeout: float = 5.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.02)
    raise CaptureError(f"timed out waiting for {path}")


def request(socket_path: Path, frame: dict[str, object], *, timeout: float = 3.0) -> dict[str, object]:
    payload = json.dumps(frame, separators=(",", ":")).encode() + b"\n"
    if len(payload) >= MAX_FRAME_BYTES:
        _fail("harness request must stay below the daemon frame limit")
    if os.name == "nt":
        with open(socket_path, "r+b", buffering=0) as connection:
            connection.write(payload)
            response = bytearray()
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline and len(response) <= MAX_FRAME_BYTES:
                chunk = connection.read(min(65536, MAX_FRAME_BYTES + 1 - len(response)))
                if not chunk:
                    break
                response.extend(chunk)
                if b"\n" in response:
                    return json.loads(bytes(response).split(b"\n", 1)[0])
        _fail("daemon closed without a JSON response")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(timeout)
        connection.connect(str(socket_path))
        connection.sendall(payload)
        response = bytearray()
        while len(response) <= MAX_FRAME_BYTES:
            chunk = connection.recv(min(65536, MAX_FRAME_BYTES + 1 - len(response)))
            if not chunk:
                break
            response.extend(chunk)
            if b"\n" in response:
                return json.loads(bytes(response).split(b"\n", 1)[0])
    _fail("daemon closed without a JSON response")


def wait_for_request(socket_path: Path, frame: dict[str, object], *, timeout: float = 5.0) -> dict[str, object]:
    deadline = time.monotonic() + timeout
    last_error: OSError | None = None
    while time.monotonic() < deadline:
        try:
            return request(socket_path, frame)
        except (ConnectionRefusedError, FileNotFoundError, TimeoutError, socket.timeout) as error:
            last_error = error
            time.sleep(0.02)
    _fail(f"timed out waiting for daemon response: {last_error}")


def load_historical_oracle(path: Path, *, repo_root: Path) -> dict[str, object]:
    """Load the pinned historical oracle without trusting caller metadata."""
    root = repo_root.resolve()
    if path.is_symlink():
        _fail("historical oracle is missing or not a regular file")
    oracle_path = path.resolve()
    if not oracle_path.is_relative_to(root):
        _fail("historical oracle escapes the repository")
    if not oracle_path.is_file():
        _fail("historical oracle is missing or not a regular file")
    if git_object_hash(repo_root, oracle_path.relative_to(root).as_posix()) != HISTORICAL_ORACLE_FIXTURE_BLOB:
        _fail("historical oracle fixture is not the pinned Git object")
    try:
        historical = json.loads(oracle_path.read_bytes())
    except (OSError, json.JSONDecodeError, UnicodeDecodeError) as error:
        _fail(f"cannot read historical oracle: {error}")
    if not isinstance(historical, dict):
        _fail("historical oracle must be a JSON object")
    if historical.get("verdict") != "INVALIDATED":
        _fail("historical oracle verdict must remain INVALIDATED")
    dependency = historical.get("dependency")
    if not isinstance(dependency, dict) or dependency.get("oracle") != HISTORICAL_ORACLE_IDENTITY:
        _fail("historical oracle identity is not pinned to Go azuretls-client v1.13.2")

    go_mod = (root / "browse/go.mod").read_text()
    match = re.search(r"(?m)^\s*github\.com/Noooste/azuretls-client\s+(\S+)", go_mod)
    if match is None or match.group(1) != HISTORICAL_AZURETLS_MODULE:
        _fail("current browse/go.mod is not bound to the historical AzureTLS module")
    provenance = (root / "browse/SOURCE_PROVENANCE.md").read_text()
    if HISTORICAL_ORACLE_SOURCE_COMMIT not in provenance:
        _fail("source provenance does not name the pinned historical oracle source commit")
    if HISTORICAL_ORACLE_FIXTURE_BLOB not in provenance:
        _fail("source provenance does not name the pinned historical oracle fixture object")

    profiles = historical.get("profiles")
    if not isinstance(profiles, list) or len(profiles) != len(FETCH_PROFILES):
        _fail("historical oracle must contain exactly six profile rows")
    seen = set()
    for row in profiles:
        if not isinstance(row, dict) or not isinstance(row.get("profile"), str):
            _fail("historical oracle contains a malformed profile row")
        profile = row["profile"]
        if profile in seen or profile not in FETCH_PROFILES:
            _fail(f"historical oracle has an unexpected or duplicate profile: {profile!r}")
        seen.add(profile)
        oracle = row.get("oracle")
        if not isinstance(oracle, dict):
            _fail(f"historical oracle row {profile} has no oracle payload")
        if oracle.get("profile") != profile:
            _fail(f"historical oracle row {profile} has a mismatched oracle profile")
        for _, historical_field in ORACLE_COMPARISON_FIELDS:
            if historical_field not in oracle:
                _fail(f"historical oracle row {profile} is missing {historical_field}")
    if seen != set(FETCH_PROFILES):
        _fail(f"historical oracle profiles do not match {list(FETCH_PROFILES)}")
    return {
        "identity": HISTORICAL_ORACLE_IDENTITY,
        "verdict": historical["verdict"],
        "source_commit": HISTORICAL_ORACLE_SOURCE_COMMIT,
        "fixture_git_blob": HISTORICAL_ORACLE_FIXTURE_BLOB,
        "module_version": HISTORICAL_AZURETLS_MODULE,
        "profiles": profiles,
    }


def git_object_hash(repo_root: Path, repo_rel_path: str) -> str:
    result = subprocess.run(
        ["git", "hash-object", "--", repo_rel_path],
        cwd=repo_root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0 or not SHA1_RE.fullmatch(result.stdout.strip()):
        _fail(f"cannot resolve Git object for {repo_rel_path}")
    return result.stdout.strip()


def compare_capture_to_historical_oracle(capture: dict, historical: dict[str, object]) -> dict[str, object]:
    """Compare current Go-client wire fields; never normalize ordering."""
    current = {row["profile"]: row for row in capture["profiles"]}
    historical_rows = {row["profile"]: row["oracle"] for row in historical["profiles"]}
    mismatches = []
    profile_results = []
    for profile in FETCH_PROFILES:
        candidate = current.get(profile)
        oracle = historical_rows.get(profile)
        if not isinstance(candidate, dict) or not isinstance(oracle, dict):
            _fail(f"capture/oracle profile set is missing {profile}")
        fields = []
        for current_field, historical_field in ORACLE_COMPARISON_FIELDS:
            actual = candidate.get(current_field)
            expected = oracle.get(historical_field)
            equal = actual == expected
            fields.append({"field": current_field, "equal": equal})
            if not equal:
                mismatches.append({
                    "profile": profile,
                    "field": current_field,
                    "expected": expected,
                    "actual": actual,
                })
        profile_results.append({"profile": profile, "equal": all(item["equal"] for item in fields), "fields": fields})
    return {
        "oracle_identity": historical["identity"],
        "oracle_source_commit": historical["source_commit"],
        "profiles": profile_results,
        "mismatches": mismatches,
        "passed": not mismatches,
    }


def rust_compat_diagnostic(
    rust_binary: Path | None, compat_binary: Path | None
) -> dict[str, object]:
    """Record why candidate binaries were not run as acceptance evidence."""
    supplied = rust_binary is not None or compat_binary is not None
    reason = (
        "not executed: supplied Rust/compat artifacts lack independently reviewed "
        "binary/source provenance"
        if supplied
        else "not executed: independent Rust/compat evidence is absent"
    )
    return {
        "executed": False,
        "passed": False,
        "reason": reason,
        "rust_artifact_supplied": rust_binary is not None,
        "compat_artifact_supplied": compat_binary is not None,
    }


def acceptance_blockers(
    historical: dict[str, object],
    oracle_comparison: dict[str, object],
    rust_comparison: dict[str, object],
) -> list[str]:
    """Return the evidence gaps that keep diagnostic comparisons non-accepting."""
    blockers = list(ACCEPTANCE_BLOCKERS)
    if historical.get("verdict") != "INVALIDATED":
        blockers[0] = "historical oracle verdict is not an accepted independent oracle"
    if oracle_comparison.get("passed"):
        blockers.append("historical oracle field equality is diagnostic only; it cannot establish parity")
    if not rust_comparison.get("executed"):
        blockers.append(str(rust_comparison["reason"]))
    else:
        blockers.append("Rust/compat wire observations remain diagnostic; native Rust transport was not exercised")
    return blockers


def compare_raw_wire(reference: dict, observed: dict) -> dict[str, object]:
    """Compare six raw captures without sorting or hiding extension order."""
    fields = (
        "protocol", "raw_client_hello_record_b64", "raw_settings_frame_b64",
        "raw_header_block_b64", "tls_client_version", "cipher_suites",
        "extensions", "supported_groups", "ec_point_formats", "h2_settings", "header_names",
    )
    reference_rows = {row["profile"]: row for row in reference["profiles"]}
    observed_rows = {row["profile"]: row for row in observed["profiles"]}
    profiles = []
    mismatches = []
    for profile in FETCH_PROFILES:
        left, right = reference_rows[profile], observed_rows[profile]
        checks = []
        for field in fields:
            equal = left.get(field) == right.get(field)
            checks.append({"field": field, "equal": equal})
            if not equal:
                mismatches.append({"profile": profile, "field": field, "reference": left.get(field), "observed": right.get(field)})
        profiles.append({"profile": profile, "equal": all(check["equal"] for check in checks), "fields": checks})
    return {
        "comparison_mode": "raw_bytes_and_declared_order",
        "note": "TLS GREASE and extension ordering are retained as observed; no list is sorted or normalized.",
        "profiles": profiles,
        "mismatches": mismatches,
        "passed": not mismatches,
    }


def _compat_socket_path(home: Path, session: str) -> Path:
    if os.name == "nt":
        return Path(r"\\.\pipe") / f"symbrowse-{session}"
    if sys.platform == "darwin":
        return home / "Library" / "Caches" / "symbrowse" / "run" / f"{session}.sock"
    return home / ".local" / "run" / "symbrowse" / f"{session}.sock"


def _terminate_process(process: subprocess.Popen[bytes]) -> None:
    """Terminate the owned process group, including descendants."""
    try:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGTERM)
        elif process.poll() is None:
            process.terminate()
        process.wait(timeout=2)
    except (OSError, subprocess.TimeoutExpired):
        try:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            elif process.poll() is None:
                process.kill()
        except OSError:
            pass
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass


def verify_go_binary_metadata(binary: Path, *, go: str) -> dict[str, str]:
    """Prove the retained compat artifact is a Go build from this module."""
    try:
        result = subprocess.run(
            [go, "version", "-m", str(binary)],
            capture_output=True,
            text=True,
            check=False,
            timeout=15,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        _fail(f"cannot inspect Go compat build info: {error}")
    if result.returncode:
        _fail("cannot inspect Go compat build info")
    lines = result.stdout.splitlines()
    if not lines or ": " not in lines[0]:
        _fail("Go compat artifact has no Go version identity")
    module_path = next(
        (line.split("\t", 2)[2] for line in lines[1:] if line.startswith("\tpath\t")),
        None,
    )
    if module_path is None or not module_path.startswith(EXPECTED_GO_MODULE):
        _fail("Go compat artifact is not built from the Browse module")
    return {"go_version": lines[0].rsplit(": ", 1)[1], "module": module_path}


def run_rust_compat_comparison(
    rust_binary: Path,
    compat_binary: Path,
    capture: dict[str, object],
    *,
    repo_root: Path,
    go: str,
) -> dict[str, object]:
    """Exercise Rust CompatClient and capture the Go sidecar's raw wire."""
    binaries = []
    for label, value in (("Rust candidate", rust_binary), ("Go compat", compat_binary)):
        if value.is_symlink():
            _fail(f"{label} binary is missing or not a regular file")
        resolved = value.resolve()
        if not resolved.is_file():
            _fail(f"{label} binary is missing or not a regular file")
        binaries.append(resolved)
    rust_sha256 = sha256_of_file(binaries[0])
    compat_sha256 = sha256_of_file(binaries[1])
    compat_build_info = verify_go_binary_metadata(binaries[1], go=go)
    if binaries[0] == binaries[1] or rust_sha256 == compat_sha256:
        _fail("Rust candidate and Go compat binaries must be distinct")

    session = f"fetch002-{os.getpid()}"
    artifact_root = repo_root / "browse/target/fetch002-wire-next"
    artifact_root.mkdir(mode=0o700, parents=True, exist_ok=True)
    base = Path(tempfile.mkdtemp(prefix="run-", dir=artifact_root))
    home, config, state, cache, tmp = (base / name for name in ("home", "config", "state", "cache", "tmp"))
    for path in (home, config, state, cache, tmp):
        path.mkdir(mode=0o700)
    sidecar_binary = Path(str(capture["executable_path"])).resolve()
    sidecar_sha256 = sha256_of_file(sidecar_binary)
    if sidecar_sha256 != capture["executable_sha256"]:
        _fail("capture test binary changed; refusing to use it as the diagnostic Go sidecar")
    sidecar_capture = base / "compat-wire.json"
    go_environment = verified_external_go_environment()
    try:
        rust_version = subprocess.run(
            [str(binaries[0]), "--version"], capture_output=True, text=True,
            check=False, timeout=15, env=go_environment,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        _fail(f"cannot record Rust candidate version before execution: {error}")
    provenance_before = {
        "recorded_before_execution": True,
        "rust_binary": str(binaries[0]),
        "rust_binary_sha256": rust_sha256,
        "rust_version_exit_code": rust_version.returncode,
        "rust_version_stdout": rust_version.stdout,
        "rust_version_stderr": rust_version.stderr,
        "compat_binary": str(binaries[1]),
        "compat_binary_sha256": compat_sha256,
        "compat_build_info": compat_build_info,
        "capture_worktree_head": capture["worktree_head"],
        "capture_executable": capture["executable_path"],
        "capture_executable_sha256": capture["executable_sha256"],
        "capture_build_input_count": len(capture["build_inputs"]),
        "source_binding_reviewed": False,
        "toolchain_environment": {name: go_environment[name] for name in (
            "GOCACHE", "GOMODCACHE", "GOTOOLCHAIN", "GOWORK", "GOPROXY", "GOFLAGS", "GOMAXPROCS",
        )},
    }
    (base / "provenance-before.json").write_text(json.dumps(provenance_before, indent=2, sort_keys=True) + "\n")
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC",
        "HOME": str(home), "TMPDIR": str(tmp), "SYMBROWSE_CONFIG_DIR": str(config),
        "SYMBROWSE_STATE_DIR": str(state), "SYMBROWSE_CACHE_DIR": str(cache),
        "SYMBROWSE_DAEMON_LOG": str(base / "daemon.log"),
        "SYMBROWSE_COMPAT_BINARY": str(sidecar_binary),
        "SYMBROWSE_FETCH_FINGERPRINTS_SIDECAR_OUT": str(sidecar_capture),
        "SYMBROWSE_FETCH_ROBOTS": "false",
    }
    env.update({name: go_environment[name] for name in (
        "GOCACHE", "GOMODCACHE", "CGO_ENABLED", "GOTOOLCHAIN", "GOWORK", "GOPROXY", "GOFLAGS", "GOMAXPROCS",
    )})
    stdout_path, stderr_path = base / "rust-daemon.stdout", base / "rust-daemon.stderr"
    with stdout_path.open("wb") as stdout_file, stderr_path.open("wb") as stderr_file:
        process = subprocess.Popen(
            [str(binaries[0]), "daemon", "--session", session, "--mode", "compat", "--allow-private"],
            cwd=repo_root / "browse", env=env, stdin=subprocess.DEVNULL,
            stdout=stdout_file, stderr=stderr_file, start_new_session=(os.name == "posix"),
        )
        socket_path = _compat_socket_path(home, session)
        results = []
        try:
            if os.name == "nt":
                wait_for_request(socket_path, {"cmd": "daemon.ping", "session": session}, timeout=10.0)
            else:
                wait_for_path(socket_path, timeout=10.0)
            for profile in FETCH_PROFILES:
                response = request(socket_path, {"cmd": "fetch.url", "session": session, "args": {
                    "url": f"https://127.0.0.1/fetch002-compat/{profile}", "profile": profile,
                    "method": "GET", "max_body_bytes": 1024 * 1024,
                }}, timeout=10.0)
                if not response.get("success"):
                    _fail(f"Rust compat request failed for {profile}: {response}")
                data = response.get("data")
                if not isinstance(data, dict) or data.get("status_code") != 200 or data.get("content") != "FETCH-002 compat loopback response\n":
                    _fail(f"Rust compat response for {profile} did not match the loopback contract: {data}")
                transport = data.get("transport")
                if not isinstance(transport, dict) or transport.get("mode") != "compat" or transport.get("tls_profile") != "azuretls-legacy":
                    _fail(f"Rust response for {profile} did not prove compat transport selection: {data}")
                results.append({"profile": profile, "status_code": data["status_code"], "transport": transport})
            stop = request(socket_path, {"cmd": "daemon.stop", "session": session}, timeout=5.0)
            if not stop.get("success"):
                _fail(f"Rust compat daemon stop failed: {stop}")
            process.wait(timeout=10)
        finally:
            _terminate_process(process)
        stdout_file.flush(); stderr_file.flush()
        if process.returncode != 0:
            _fail(f"Rust compat daemon exited with status {process.returncode}")
        if stdout_path.read_bytes():
            _fail("Rust compat daemon wrote non-protocol bytes to stdout")
        if len(stderr_path.read_bytes()) > 65536:
            _fail("Rust compat daemon stderr exceeded the harness bound")
        if os.name != "nt" and socket_path.exists():
            _fail("Rust compat daemon socket survived clean shutdown")
    if not sidecar_capture.is_file():
        _fail("Go diagnostic sidecar did not retain its six-profile wire capture")
    observed = json.loads(sidecar_capture.read_bytes())
    validate_capture(observed, repo_root=repo_root, go=go)
    if sha256_of_file(sidecar_binary) != sidecar_sha256:
        _fail("Go diagnostic sidecar binary changed during the comparison")
    if sha256_of_file(binaries[0]) != rust_sha256 or sha256_of_file(binaries[1]) != compat_sha256:
        _fail("Rust or Go compat binary changed during the comparison")
    return {
        "executed": True,
        "passed": True,
        "profiles": results,
        "request_url": "loopback-only",
        "rust_binary_sha256": rust_sha256,
        "compat_binary_sha256": compat_sha256,
        "compat_build_info": compat_build_info,
        "wire_sidecar_binary_sha256": sidecar_sha256,
        "wire_capture_path": str(sidecar_capture),
        "wire_capture": compare_raw_wire(capture, observed),
        "artifacts": str(base),
    }


def verify_artifacts(capture: dict, *, evidence_root: Path, go: str) -> None:
    """Check real binary bytes/build info; metadata alone is not evidence."""
    binary = Path(capture["executable_path"])
    root = evidence_root.resolve()
    if binary.is_symlink():
        _fail("capture executable is missing or not a regular file")
    if not binary.resolve().is_relative_to(root):
        _fail("executable_path escapes the evidence directory")
    if not binary.is_file():
        _fail("capture executable is missing or not a regular file")
    if sha256_of_file(binary) != capture["executable_sha256"]:
        _fail("capture executable sha256 mismatch")
    try:
        result = subprocess.run(
            [go, "version", "-m", str(binary)], capture_output=True,
            text=True, check=False, timeout=15,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        _fail(f"cannot inspect executable build info: {error}")
    if result.returncode:
        _fail("cannot inspect executable build info")
    lines = result.stdout.splitlines()
    if not lines or ": " not in lines[0]:
        _fail("executable has no Go version identity")
    settings = {}
    for line in lines[1:]:
        fields = line.strip().split("\t")
        if len(fields) == 2 and fields[0] == "build" and "=" in fields[1]:
            key, value = fields[1].split("=", 1)
            settings[key] = value
    actual = {"go_version": lines[0].rsplit(": ", 1)[1],
              "goos": settings.get("GOOS"), "goarch": settings.get("GOARCH")}
    if capture["compiler"] != actual:
        _fail("compiler identity does not match executable build info")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture", required=True, type=Path, help="path to the capture JSON file")
    parser.add_argument("--trusted-sha256", required=True,
                        help="independently reviewed digest; never derive it from this input at acceptance time")
    parser.add_argument("--go", default="go", help="Go tool used to read binary build metadata")
    parser.add_argument("--historical-oracle", type=Path,
                        help="pinned historical FETCH-002 oracle (defaults to the tracked overlay)")
    parser.add_argument("--rust", type=Path, help="retained Rust symbrowse binary for the compat call")
    parser.add_argument("--compat", type=Path, help="retained Go symbrowse-compat binary")
    parser.add_argument("--report", type=Path, help="optional path for the machine-readable comparison report")
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[3],
        help="repository root to resolve git/working-tree paths against",
    )
    args = parser.parse_args()

    if not args.capture.is_file():
        print(f"error: capture file not found: {args.capture}", file=sys.stderr)
        return 1
    if (args.rust is None) != (args.compat is None):
        parser.error("--rust and --compat must be supplied together")
    try:
        if not SHA256_RE.fullmatch(args.trusted_sha256):
            _fail("trusted-sha256 must be exactly 64 lowercase hex characters")
        raw = args.capture.read_bytes()
        capture = json.loads(raw)
        validate_capture(capture, repo_root=args.repo_root, go=args.go)
        verify_artifacts(capture, evidence_root=args.capture.parent, go=args.go)
        if hashlib.sha256(raw).hexdigest() != args.trusted_sha256:
            _fail("capture does not match the independently trusted sha256")
        historical_path = args.historical_oracle or (args.repo_root / HISTORICAL_ORACLE_REL_PATH)
        historical = load_historical_oracle(historical_path, repo_root=args.repo_root)
        oracle_comparison = compare_capture_to_historical_oracle(capture, historical)
        compat_comparison = (
            run_rust_compat_comparison(args.rust, args.compat, capture, repo_root=args.repo_root, go=args.go)
            if args.rust is not None and args.compat is not None
            else rust_compat_diagnostic(args.rust, args.compat)
        )
        blockers = acceptance_blockers(historical, oracle_comparison, compat_comparison)
        report = {
            "schema_version": 1,
            "capture_sha256": hashlib.sha256(raw).hexdigest(),
            "oracle": {
                "identity": historical["identity"],
                "source_commit": historical["source_commit"],
                "fixture_git_blob": historical["fixture_git_blob"],
                "module_version": historical["module_version"],
            },
            "oracle_comparison": oracle_comparison,
            "rust_compat_comparison": compat_comparison,
            "comparison_completed": bool(compat_comparison.get("executed")),
            "parity_established": False,
            "acceptance_blockers": blockers,
        }
        if args.report:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    except (OSError, CaptureError, json.JSONDecodeError, UnicodeDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print("FETCH-002 acceptance blocked; diagnostics were retained:", file=sys.stderr)
    for blocker in report["acceptance_blockers"]:
        print(f"- {blocker}", file=sys.stderr)
    if args.report:
        print(f"report: {args.report}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
