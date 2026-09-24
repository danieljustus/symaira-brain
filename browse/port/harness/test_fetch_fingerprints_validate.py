#!/usr/bin/env python3
"""Unit tests ONLY for fetch_fingerprints_validate.py's mutation rejection.

Wire bytes (ClientHello/SETTINGS/HEADERS) here are hand-built synthetic
fixtures, not a real capture. They are never FETCH-002 evidence themselves —
only `fetch_fingerprints_validate.py` run against a real capture produced by
`go test ./internal/fetch/fetch/ -run TestGenerateFetchFingerprintsCapture`
is evidence. This file only proves the validator rejects a compact table of
realistic mutations of an otherwise-valid capture. Build-input source
bindings (sha256/git blob) reference this real repository's real files,
since that binding logic is exactly what is under test.
"""
from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import os
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location(
    "fetch_fingerprints_validate", ROOT / "browse/port/harness/fetch_fingerprints_validate.py"
)
assert SPEC and SPEC.loader
validate_mod = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validate_mod
SPEC.loader.exec_module(validate_mod)

CURRENT_HEAD = subprocess.run(
    ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
).stdout.strip()


# ---------------------------------------------------------------------------
# Minimal HPACK encoder, used only to build synthetic fixtures for these
# tests. It is the deliberate inverse of the decoder under test, written
# independently (not by calling into validate_mod's internals) so a bug in
# one is not masked by reusing it in the other.
# ---------------------------------------------------------------------------


def _hpack_encode_int(value: int, prefix_bits: int, first_byte_bits: int) -> bytes:
    max_prefix = (1 << prefix_bits) - 1
    if value < max_prefix:
        return bytes([first_byte_bits | value])
    out = bytearray([first_byte_bits | max_prefix])
    value -= max_prefix
    while value >= 128:
        out.append((value % 128) + 128)
        value //= 128
    out.append(value)
    return bytes(out)


def _hpack_indexed(index: int) -> bytes:
    return _hpack_encode_int(index, 7, 0x80)


def _hpack_literal_indexed_name(index: int, value: str) -> bytes:
    head = _hpack_encode_int(index, 4, 0x00)  # "without indexing", indexed name
    value_bytes = value.encode("ascii")
    return head + _hpack_encode_int(len(value_bytes), 7, 0x00) + value_bytes


def _synthetic_header_block() -> bytes:
    return (
        _hpack_indexed(2)  # :method: GET
        + _hpack_literal_indexed_name(1, "127.0.0.1:9")  # :authority
        + _hpack_indexed(7)  # :scheme: https
        + _hpack_literal_indexed_name(4, "/capture")  # :path
        + _hpack_literal_indexed_name(16, "identity")  # accept-encoding
        + _hpack_literal_indexed_name(58, "test-agent")  # user-agent
    )


_SYNTHETIC_HEADER_NAMES = [":method", ":authority", ":scheme", ":path", "accept-encoding", "user-agent"]


def _synthetic_settings_frame() -> bytes:
    settings = [(1, 65536), (2, 0), (4, 6291456), (6, 262144)]
    return b"".join(struct.pack(">HI", sid, val) for sid, val in settings)


# A minimal, hand-built (not captured) ClientHello record: version 0x0303, a
# 32-byte random, no session id, cipher suites (one GREASE + one real), no
# compression methods, and no extensions block.
def _synthetic_client_hello() -> bytes:
    body = (
        bytes([0x03, 0x03])
        + bytes(32)
        + bytes([0x00])  # session_id length
        + bytes([0x00, 0x04, 0x0A, 0x0A, 0x13, 0x01])  # 2 cipher suites: GREASE 0x0A0A, TLS_AES_128_GCM_SHA256
        + bytes([0x00])  # compression methods length
    )
    handshake = bytes([0x01]) + len(body).to_bytes(3, "big") + body
    return bytes([0x16, 0x03, 0x01]) + len(handshake).to_bytes(2, "big") + handshake


def _real_build_inputs() -> list[dict]:
    entries = []
    for full_string in sorted(validate_mod.compiler_input_closure(str(ROOT), "go")):
        full = Path(full_string)
        path = full.relative_to(ROOT).as_posix() if full.is_relative_to(ROOT) else str(full)
        entry = {"path": path, "sha256": validate_mod.sha256_of_file(full)}
        blob = validate_mod.git_blob_at_head(ROOT, path) if full.is_relative_to(ROOT) else None
        if blob is not None:
            entry["git_blob_at_head"] = blob
        entries.append(entry)
    return entries


def _synthetic_profile_case(profile: str) -> dict:
    raw_hello = _synthetic_client_hello()
    fields = validate_mod.reparse_client_hello(raw_hello)
    ja3_string = validate_mod._ja3_string(
        fields["tls_client_version"], fields["cipher_suites"], fields["extensions"],
        fields["supported_groups"], fields["ec_point_formats"],
    )
    no_grease_ciphers = validate_mod._filter_grease(fields["cipher_suites"])
    no_grease_ja3_string = validate_mod._ja3_string(
        fields["tls_client_version"], no_grease_ciphers, fields["extensions"],
        fields["supported_groups"], fields["ec_point_formats"],
    )
    settings_frame = _synthetic_settings_frame()
    header_block = _synthetic_header_block()
    import base64

    return {
        "profile": profile,
        "protocol": "h2",
        "raw_client_hello_record_b64": base64.b64encode(raw_hello).decode(),
        "raw_settings_frame_b64": base64.b64encode(settings_frame).decode(),
        "raw_header_block_b64": base64.b64encode(header_block).decode(),
        "ja3_string": ja3_string,
        "ja3": hashlib.md5(ja3_string.encode()).hexdigest(),
        "cipher_suites_no_grease": no_grease_ciphers,
        "extensions_no_grease": fields["extensions"],
        "supported_groups_no_grease": fields["supported_groups"],
        "ja3_string_no_grease": no_grease_ja3_string,
        "ja3_no_grease": hashlib.md5(no_grease_ja3_string.encode()).hexdigest(),
        "h2_settings": validate_mod.reparse_settings_frame(settings_frame),
        "header_names": list(_SYNTHETIC_HEADER_NAMES),
        **fields,
    }


def _valid_capture() -> dict:
    return {
        "schema_version": 2,
        "capture_kind": "loopback_hermetic_production_client",
        "note": "synthetic unit-test fixture, not a real capture",
        "worktree_head": CURRENT_HEAD,
        "compiler": {"go_version": "go1.26.6", "goos": "darwin", "goarch": "arm64"},
        "executable_sha256": "0" * 64,
        "executable_path": "/tmp/synthetic-test-binary",
        "build_inputs": _real_build_inputs(),
        "captured_at_utc": "2026-01-01T00:00:00Z",
        "profiles": [_synthetic_profile_case(name) for name in sorted(validate_mod.SIX_PROFILES)],
    }


def _mutate_stale_head(capture: dict) -> dict:
    capture["worktree_head"] = "0" * 40
    return capture


def _mutate_bool_schema_version(capture: dict) -> dict:
    capture["schema_version"] = True
    return capture


def _mutate_missing_build_input(capture: dict) -> dict:
    capture["build_inputs"] = capture["build_inputs"][:-1]
    return capture


def _mutate_extra_build_input(capture: dict) -> dict:
    extra_path = "browse/port/harness/test_fetch_fingerprints_validate.py"
    entry = {"path": extra_path, "sha256": validate_mod.sha256_of_file(ROOT / extra_path)}
    blob = validate_mod.git_blob_at_head(ROOT, extra_path)
    if blob is not None:
        entry["git_blob_at_head"] = blob
    capture["build_inputs"].append(entry)
    return capture


def _mutate_wrong_sha256(capture: dict) -> dict:
    capture["build_inputs"][0]["sha256"] = "f" * 64
    return capture


def _mutate_wrong_git_blob(capture: dict) -> dict:
    entry = next(e for e in capture["build_inputs"] if "git_blob_at_head" in e)
    entry["git_blob_at_head"] = "f" * 40
    return capture


def _mutate_missing_profile(capture: dict) -> dict:
    capture["profiles"] = capture["profiles"][:-1]
    return capture


def _mutate_duplicate_profile(capture: dict) -> dict:
    capture["profiles"][-1] = _synthetic_profile_case(capture["profiles"][0]["profile"])
    return capture


def _mutate_unknown_profile(capture: dict) -> dict:
    capture["profiles"][-1]["profile"] = "brave"
    return capture


def _mutate_list_profile_field(capture: dict) -> dict:
    capture["profiles"][-1]["profile"] = []
    return capture


def _mutate_tampered_raw_hello(capture: dict) -> dict:
    import base64

    raw = bytearray(base64.b64decode(capture["profiles"][0]["raw_client_hello_record_b64"]))
    # Mutate a cipher-suite byte, not the deliberately unparsed TLS random.
    raw[raw.index(bytes.fromhex("0a0a1301")) + 2] ^= 0xFF
    capture["profiles"][0]["raw_client_hello_record_b64"] = base64.b64encode(bytes(raw)).decode()
    return capture


def _mutate_ja3_mismatch(capture: dict) -> dict:
    capture["profiles"][0]["ja3"] = "0" * 32
    return capture


def _mutate_grease_not_filtered(capture: dict) -> dict:
    # Claim the raw (GREASE-included) cipher list as the "stable" one.
    capture["profiles"][0]["cipher_suites_no_grease"] = capture["profiles"][0]["cipher_suites"]
    return capture


def _mutate_invented_h2_settings(capture: dict) -> dict:
    capture["profiles"][0]["h2_settings"] = ["1=99999", "2=1"]
    return capture


def _mutate_invented_header_names(capture: dict) -> dict:
    capture["profiles"][0]["header_names"] = [":method", ":path"]
    return capture


def _mutate_reported_error_field(capture: dict) -> dict:
    capture["profiles"][0]["request_error"] = "connection reset"
    return capture


def _mutate_empty_header_names(capture: dict) -> dict:
    capture["profiles"][0]["header_names"] = []
    capture["profiles"][0]["raw_header_block_b64"] = ""
    return capture


def _mutate_missing_field(capture: dict) -> dict:
    del capture["profiles"][0]["h2_settings"]
    return capture


# One compact table: (label, mutation, expected substring in the rejection).
MUTATIONS = [
    ("stale worktree head", _mutate_stale_head, "stale capture"),
    ("bool masquerading as schema_version 2", _mutate_bool_schema_version, "schema_version"),
    ("missing required build input", _mutate_missing_build_input, "does not match the required closure"),
    ("extra unrequired build input", _mutate_extra_build_input, "unexpected path"),
    ("wrong build-input sha256", _mutate_wrong_sha256, "sha256 mismatch"),
    ("wrong build-input git blob", _mutate_wrong_git_blob, "git blob mismatch"),
    ("missing profile", _mutate_missing_profile, "exactly the six profiles"),
    ("duplicate profile", _mutate_duplicate_profile, "exactly the six profiles"),
    ("unknown profile id", _mutate_unknown_profile, "unknown or non-string profile"),
    ("non-hashable profile field", _mutate_list_profile_field, "unknown or non-string profile"),
    ("tampered raw ClientHello bytes", _mutate_tampered_raw_hello, "does not match an independent re-parse"),
    ("ja3 no longer matches ja3_string", _mutate_ja3_mismatch, "internally inconsistent summary"),
    ("GREASE-included list claimed as stable", _mutate_grease_not_filtered, "GREASE-filtered"),
    ("invented h2_settings not on the wire", _mutate_invented_h2_settings, "independent re-parse"),
    ("invented header_names not on the wire", _mutate_invented_header_names, "independent HPACK re-derivation"),
    ("silently-accepted reported error field", _mutate_reported_error_field, "field mismatch"),
    ("empty header_names", _mutate_empty_header_names, "header_names must be non-empty"),
    ("missing required field", _mutate_missing_field, "field mismatch"),
]


class FetchFingerprintsValidateMutationTests(unittest.TestCase):
    def test_compiler_closure_decodes_go_json_as_utf8(self) -> None:
        package = {"Dir": str(ROOT / "browse"), "GoFiles": ["go.mod"]}
        result = subprocess.CompletedProcess([], 0, json.dumps(package), "")
        validate_mod.compiler_input_closure.cache_clear()
        with patch.object(validate_mod, "verified_external_go_environment", return_value={}), \
             patch.object(validate_mod.subprocess, "run", return_value=result) as run:
            validate_mod.compiler_input_closure(str(ROOT), "unit-only-go")
        self.assertEqual(run.call_args.kwargs["encoding"], "utf-8")

    def test_artifact_identity(self) -> None:
        # Synthetic unit-only binary; only build-info parsing is mocked.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "capture.test"
            binary.write_bytes(b"unit-only binary bytes")
            capture = {"executable_path": str(binary),
                       "executable_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                       "compiler": {"go_version": "go1.26.6", "goos": "darwin", "goarch": "arm64"}}
            package_path = f"{validate_mod.EXPECTED_GO_MODULE}/internal/fetch/fetch.test"
            result = subprocess.CompletedProcess(
                [], 0,
                f"{binary}: go1.26.6\n\tpath\t{package_path}\n"
                "\tbuild\tGOOS=darwin\n\tbuild\tGOARCH=arm64\n",
                "",
            )
            with patch.object(validate_mod.subprocess, "run", return_value=result):
                validate_mod.verify_artifacts(capture, evidence_root=root, go="unit-only-go")
                for field, value, message in [
                    ("executable_sha256", "0" * 64, "sha256 mismatch"),
                    ("executable_path", str(root / "missing"), "missing"),
                    ("executable_path", str(root.parent / "outside"), "escapes"),
                    ("compiler", {"go_version": "fictional", "goos": "darwin", "goarch": "arm64"}, "compiler identity"),
                ]:
                    with self.subTest(field=field, value=value):
                        changed = dict(capture, **{field: value})
                        with self.assertRaisesRegex(validate_mod.CaptureError, message):
                            validate_mod.verify_artifacts(changed, evidence_root=root, go="unit-only-go")
            wrong_package = subprocess.CompletedProcess(
                [], 0,
                f"{binary}: go1.26.6\n\tpath\t{validate_mod.EXPECTED_GO_MODULE}/cmd/symbrowse\n"
                "\tbuild\tGOOS=darwin\n\tbuild\tGOARCH=arm64\n",
                "",
            )
            with patch.object(validate_mod.subprocess, "run", return_value=wrong_package):
                with self.assertRaisesRegex(validate_mod.CaptureError, "not the pinned FETCH-002 Go test package"):
                    validate_mod.verify_artifacts(capture, evidence_root=root, go="unit-only-go")

    def test_compat_build_identity_is_checked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "symbrowse-compat"
            binary.write_bytes(b"unit-only compat artifact")
            output = f"{binary}: go1.26.6\n\tpath\t{validate_mod.EXPECTED_GO_MODULE}/cmd/symbrowse\n"
            result = subprocess.CompletedProcess([], 0, output, "")
            with patch.object(validate_mod.subprocess, "run", return_value=result):
                metadata = validate_mod.verify_go_binary_metadata(binary, go="unit-only-go")
            self.assertEqual(metadata["module"], f"{validate_mod.EXPECTED_GO_MODULE}/cmd/symbrowse")
            bad = subprocess.CompletedProcess([], 0, f"{binary}: go1.26.6\n\tpath\tother/module\n", "")
            with patch.object(validate_mod.subprocess, "run", return_value=bad):
                with self.assertRaisesRegex(validate_mod.CaptureError, "not built from the Browse module"):
                    validate_mod.verify_go_binary_metadata(binary, go="unit-only-go")

    def test_accepts_well_formed_synthetic_capture(self) -> None:
        validate_mod.validate_capture(_valid_capture(), repo_root=ROOT)  # must not raise

    def test_historical_oracle_is_source_bound(self) -> None:
        oracle_path = ROOT / validate_mod.HISTORICAL_ORACLE_REL_PATH
        historical = validate_mod.load_historical_oracle(oracle_path, repo_root=ROOT)
        self.assertEqual(historical["identity"], "Go azuretls-client v1.13.2")
        self.assertEqual(historical["fixture_git_blob"], validate_mod.HISTORICAL_ORACLE_FIXTURE_BLOB)
        comparison = validate_mod.compare_capture_to_historical_oracle(_valid_capture(), historical)
        self.assertFalse(comparison["passed"])
        self.assertTrue(comparison["mismatches"])

    def test_chrome_family_extension_shuffle_is_membership_only(self) -> None:
        historical = validate_mod.load_historical_oracle(
            ROOT / validate_mod.HISTORICAL_ORACLE_REL_PATH, repo_root=ROOT
        )
        historical_rows = {row["profile"]: row["oracle"] for row in historical["profiles"]}
        candidate = {"profiles": []}
        for profile in validate_mod.FETCH_PROFILES:
            oracle = historical_rows[profile]
            candidate["profiles"].append({
                "profile": profile,
                "protocol": oracle["protocol"],
                "tls_client_version": oracle["tls_version"],
                "cipher_suites_no_grease": oracle["cipher_suites"],
                "extensions_no_grease": list(oracle["extensions"]),
                "h2_settings": oracle["h2_settings"],
                "header_names": oracle["header_names"],
            })
        randomized = {"chrome", "edge", "opera"}
        for row in candidate["profiles"]:
            if row["profile"] in randomized:
                row["extensions_no_grease"].reverse()

        comparison = validate_mod.compare_capture_to_historical_oracle(candidate, historical)
        self.assertTrue(comparison["passed"])
        self.assertEqual(comparison["extension_order_relaxed_profiles"], ["chrome", "edge", "opera"])
        for row in comparison["profiles"]:
            extension_field = next(field for field in row["fields"] if field["field"] == "extensions_no_grease")
            self.assertTrue(extension_field["equal"])
            if row["profile"] in randomized:
                self.assertEqual(extension_field["comparison"], "unordered_multiset")
                self.assertFalse(extension_field["wire_order_equal"])

        blockers = validate_mod.acceptance_blockers(
            historical, comparison, {"executed": False, "passed": False, "reason": "Rust diagnostic absent"}
        )
        self.assertTrue(any("historical oracle verdict is INVALIDATED" in item for item in blockers))

        # Negative controls: reordered deterministic Firefox extensions and a
        # missing Chrome extension remain concrete mismatches.
        negative = copy.deepcopy(candidate)
        firefox = next(row for row in negative["profiles"] if row["profile"] == "firefox")
        firefox["extensions_no_grease"].reverse()
        missing_chrome = next(row for row in negative["profiles"] if row["profile"] == "chrome")
        missing_chrome["extensions_no_grease"].pop()
        rejected = validate_mod.compare_capture_to_historical_oracle(negative, historical)
        self.assertFalse(rejected["passed"])
        self.assertEqual(
            {(item["profile"], item["field"]) for item in rejected["mismatches"]},
            {("firefox", "extensions_no_grease"), ("chrome", "extensions_no_grease")},
        )

    def test_invalidated_oracle_match_never_establishes_parity(self) -> None:
        historical = validate_mod.load_historical_oracle(
            ROOT / validate_mod.HISTORICAL_ORACLE_REL_PATH, repo_root=ROOT
        )
        blockers = validate_mod.acceptance_blockers(
            historical,
            {"passed": True},
            {"executed": False, "passed": False, "reason": "Rust comparison absent"},
        )
        self.assertEqual(historical["verdict"], "INVALIDATED")
        self.assertIn("historical oracle verdict is INVALIDATED", blockers[0])
        self.assertTrue(any("raw-wire comparison is diagnostic only" in blocker for blocker in blockers))
        self.assertTrue(any("historical oracle field equality is diagnostic only" in blocker for blocker in blockers))
        self.assertTrue(any("Rust comparison absent" in blocker for blocker in blockers))
        self.assertTrue(blockers)

    def test_missing_rust_evidence_cannot_pass_or_launch_binaries(self) -> None:
        status = validate_mod.rust_compat_diagnostic(None, None)
        self.assertFalse(status["executed"])
        self.assertFalse(status["passed"])
        blockers = validate_mod.acceptance_blockers(
            {"verdict": "INVALIDATED"}, {"passed": False}, status
        )
        self.assertTrue(blockers)
        self.assertIn("independent Rust/compat evidence is absent", blockers[-1])
        with patch.object(validate_mod.subprocess, "Popen") as launch:
            validate_mod.rust_compat_diagnostic(None, None)
        launch.assert_not_called()

    def test_runtime_root_shortens_socket_and_long_paths_fail_fast(self) -> None:
        with tempfile.TemporaryDirectory(prefix="fetch002-runtime-", dir=validate_mod.EXTERNAL_RUNTIME_ROOT) as directory:
            runtime_root = Path(directory)
            selected = validate_mod._runtime_home(ROOT / "deep/home", runtime_root)
            socket_path = validate_mod._compat_socket_path(selected, "fetch002-123")
            self.assertEqual(selected, runtime_root.resolve())
            self.assertLess(len(os.fsencode(socket_path)), 104)

        with self.assertRaisesRegex(validate_mod.CaptureError, "--runtime-root must be under"):
            validate_mod._runtime_home(ROOT / "deep/home", Path("/tmp/fetch002-runtime"))

        long_socket = Path("/" + ("x" * 120) + ".sock")
        with patch.object(validate_mod.sys, "platform", "darwin"):
            with self.assertRaisesRegex(validate_mod.CaptureError, "rerun with --runtime-root"):
                validate_mod._validate_socket_path(long_socket)

    def test_runtime_root_rejects_unmounted_external_volume(self) -> None:
        with tempfile.TemporaryDirectory(prefix="fetch002-volume-", dir=validate_mod.EXTERNAL_RUNTIME_ROOT) as directory:
            unmounted = Path(directory) / "unmounted-volume"
            child = unmounted / "runtime"
            with patch.object(validate_mod, "EXTERNAL_RUNTIME_ROOT", unmounted):
                with self.assertRaisesRegex(validate_mod.CaptureError, "mounted filesystem"):
                    validate_mod._runtime_home(ROOT / "deep/home", child)
            self.assertFalse(child.exists())

    def test_waits_for_parseable_sidecar_capture(self) -> None:
        with tempfile.TemporaryDirectory(prefix="fetch002-sidecar-", dir=validate_mod.EXTERNAL_RUNTIME_ROOT) as directory:
            path = Path(directory) / "compat-wire.json"
            writes = iter((b"{", b"{}"))

            def write_next(_: float) -> None:
                path.write_bytes(next(writes))

            with patch.object(validate_mod.time, "monotonic", side_effect=(0.0, 0.0, 0.1, 0.2)), \
                 patch.object(validate_mod.time, "sleep", side_effect=write_next):
                validate_mod.wait_for_sidecar_capture(path, timeout=1.0)

    def test_rejects_unpinned_oracle_copy(self) -> None:
        source = ROOT / validate_mod.HISTORICAL_ORACLE_REL_PATH
        with tempfile.TemporaryDirectory(
            prefix="fetch002-next-oracle-", dir=ROOT / "browse/target"
        ) as directory:
            copy_path = Path(directory) / source.name
            copy_path.write_bytes(source.read_bytes() + b"\\n")
            with self.assertRaisesRegex(validate_mod.CaptureError, "pinned Git object"):
                validate_mod.load_historical_oracle(copy_path, repo_root=ROOT)

    def test_source_bound_report_keeps_oracle_invalidated_and_parity_unclaimed(self) -> None:
        raw = b"source-bound capture bytes"
        capture = {
            "capture_kind": "loopback_hermetic_production_client",
            "worktree_head": "a" * 40,
            "compiler": {"go_version": "go1.26.6", "goos": "linux", "goarch": "amd64"},
            "executable_sha256": "b" * 64,
            "build_inputs": [{"path": "browse/go.mod"}],
            "profiles": [{"profile": profile} for profile in validate_mod.FETCH_PROFILES],
        }
        report = validate_mod.source_bound_capture_report(capture, raw)
        self.assertEqual(report["capture_sha256_self_measured"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(report["historical_oracle_status"], "INVALIDATED")
        self.assertFalse(report["current_source_bound_oracle_established"])
        self.assertFalse(report["native_rust_parity_compared"])
        self.assertEqual(len(report["wire_validation"]["profiles"]), 6)
        capture["capture_kind"] = "loopback_hermetic_compat_sidecar"
        with self.assertRaisesRegex(validate_mod.CaptureError, "production_client capture"):
            validate_mod.source_bound_capture_report(capture, raw)

    def test_ci_go_environment_does_not_require_local_nvme_cache_vars(self) -> None:
        with patch.dict(os.environ, {"CI": "true"}, clear=True):
            environment = validate_mod.verified_external_go_environment()
        self.assertEqual(environment["GOPROXY"], "off")
        self.assertEqual(environment["GOWORK"], "off")
        self.assertEqual(environment["GOFLAGS"], "-mod=readonly")

    def test_rejects_every_mutation(self) -> None:
        self.assertEqual(len(MUTATIONS), 18, "update this count when the table changes")
        for label, mutate, expected_substring in MUTATIONS:
            with self.subTest(label):
                capture = mutate(copy.deepcopy(_valid_capture()))
                with self.assertRaises(validate_mod.CaptureError) as ctx:
                    validate_mod.validate_capture(capture, repo_root=ROOT)
                self.assertIn(expected_substring, str(ctx.exception), label)


if __name__ == "__main__":
    unittest.main()
