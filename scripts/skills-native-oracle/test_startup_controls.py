"""Synthetic structural controls, NOT Go/Rust or native Windows evidence."""
from copy import deepcopy
import hashlib
import json
import ntpath
import os
from pathlib import Path
import tempfile
import struct
import unittest
from unittest.mock import patch

from compare import matched as frozen_matched
from fixtures import PROFILE, write
from json_compare import json_matched
from ordinary_startup import BINDINGS, bind, matched, pair, assert_control_bootstrap
from run import compare_pair
from startup_controls import MEMORY_KEY, MEMORY_DB
from startup_privacy import private, validate_windows


def add_file(record, path, content, mode=0o600):
    digest = hashlib.sha256(content).hexdigest()
    record["filesystem"]["files"][path] = {"type": "file", "mode": mode, "value": digest}
    record["filesystem"]["raw_file_hashes"][path] = {"sha256": digest, "bytes": len(content)}


def synthetic_acl():
    user = "S-1-5-21-100-200-300-1000"
    row = {"owner_sid": user, "user_sid": user, "dacl_present": True, "control_flags": 0x8004,
            "aces": [{"type": 0, "flags": 0, "mask": 0x1f01ff, "sid": sid}
                     for sid in (user, "S-1-5-18", "S-1-5-32-544")]}
    seal_descriptor(row)
    return row


def seal_descriptor(row):
    """Synthetic Windows binary grammar control, never native execution."""
    def sid(value):
        fields = [int(part) for part in value.split('-')[1:]]
        revision, authority, *parts = fields
        return bytes((revision, len(parts))) + authority.to_bytes(6, 'big') + struct.pack(f'<{len(parts)}I', *parts)
    owner = sid(row['owner_sid'])
    aces = []
    for ace in row['aces']:
        identity = sid(ace['sid'])
        aces.append(struct.pack('<BBHi', ace['type'], ace['flags'], 8 + len(identity), ace['mask']) + identity)
    acl = struct.pack('<BBHHH', 2, 0, 8 + sum(map(len, aces)), len(aces), 0) + b''.join(aces)
    row['descriptor_hex'] = (struct.pack('<BBHIIII', 1, 0, row['control_flags'], 20, 0, 0, 20 + len(owner)) + owner + acl).hex()


def attach_synthetic_privacy(record):
    root = 'C:\\synthetic-owned'
    entries = {path: dict(synthetic_acl(), path=ntpath.join(root, *path.split('/'))) for path in sorted(record['filesystem']['files'])}
    record['cwd'] = ntpath.join(root, 'project')
    record['startup_privacy'] = {'source_class': 'synthetic policy record', 'root': root,
        'paths': sorted(entries), 'mechanism': 'windows-native-owner-dacl', 'exit': 0, 'stderr_hex': '',
        'stdout_hex': json.dumps(list(entries.values())).encode().hex(), 'entries': entries}


def synthetic_pair(parse_error=False, skills=True):
    responses = [
        {"jsonrpc": "2.0", "id": 1, "result": {"serverInfo": {"name": "symbrain", "version": "dev"}}},
        {"jsonrpc": "2.0", "id": 2, "result": {"tools": [{"name": "bootstrap"}, {"name": "patterns"}] +
         ([{"name": f"skills_{index}", "inputSchema": {"type": "object"}} for index in range(11)] if skills else [])}},
        {"jsonrpc": "2.0", "id": 3, "result": {"isError": False, "content": [{"type": "text", "text": '{"ok":true}'}]}},
    ]
    if parse_error:
        responses[2] = {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "Parse error: invalid character ' ' in exponent of numeric literal"}}
    elif not skills:
        responses[2] = {"jsonrpc": "2.0", "id": 3, "error": {"code": -32601, "message": "Tool not found"}}
    record = {"exit": 0, "stdout_hex": b"\n".join(json.dumps(row).encode() for row in responses).hex(),
              "stderr_hex": "", "startup_input": dict(BINDINGS[0 if skills else 1]), "expected_skills": 11 if skills else 0,
              "filesystem": {"files": {}, "raw_file_hashes": {}, "validated_skl004_locks": {}}}
    for path in ("data/symbrain", "data/symbrain/skills", "cache/symbrain/skills"):
        record["filesystem"]["files"][path] = {"type": "directory", "mode": 0o755}
    add_file(record, "cache/symbrain/skills/marker.json", b"synthetic marker", 0o644)
    rust = deepcopy(record)
    go = deepcopy(record)
    called = not parse_error and skills
    for path in ("config/symbrain", "config/symbrain/memory", "data/symbrain/memory", "data/symbrain/audit"):
        go["filesystem"]["files"][path] = {"type": "directory", "mode": 0o700}
    add_file(go, MEMORY_KEY, b"0" * 64 + b"\n")
    add_file(go, MEMORY_DB, b"synthetic database; not an oracle capture")
    add_file(go, "data/symbrain/audit/skills-native-764.jsonl", b"synthetic audit\n" if called else b"")
    if called:
        go["filesystem"]["files"]["data/symbrain/recipes"] = {"type": "directory", "mode": 0o700}
        add_file(go, "data/symbrain/recipes/skills-native-764.jsonl", b"synthetic episode\n")
    if os.name == "nt":
        for record in (go, rust):
            attach_synthetic_privacy(record)
    return go, rust


class OrdinaryStartupTest(unittest.TestCase):
    def test_coordinated_hash_and_byte_type_mutations_fail_entrypoint(self):
        for mutation in ('nonhex', 'uppercase', 'boolean-bytes', 'string-bytes', 'negative-bytes'):
            with self.subTest(mutation=mutation):
                go, rust = synthetic_pair()
                root = Path('.')
                self.assertTrue(compare_pair({'id': 'mcp-ordinary', 'mcp': True, 'go': go, 'rust': rust}, root))
                raw = go['filesystem']['raw_file_hashes'][MEMORY_KEY]
                if mutation in ('nonhex', 'uppercase'):
                    value = 'z' * 64 if mutation == 'nonhex' else 'A' * 64
                    raw['sha256'] = go['filesystem']['files'][MEMORY_KEY]['value'] = value
                else:
                    raw['bytes'] = {'boolean-bytes': True, 'string-bytes': '65', 'negative-bytes': -1}[mutation]
                with self.assertRaises(AssertionError):
                    compare_pair({'id': 'mcp-ordinary', 'mcp': True, 'go': go, 'rust': rust}, root)

    def test_ordinary_entrypoint_classifies_without_mutating_raw_records(self):
        for parse_error, skills in ((False, True), (True, True), (False, False)):
            with self.subTest(parse_error=parse_error, skills=skills):
                go, rust = synthetic_pair(parse_error, skills)
                original = deepcopy((go, rust))
                identity = "json-eof-meta-exp-plus-line" if parse_error else "mcp-ordinary"
                row = {"id": identity, "mcp": True, "go": go, "rust": rust}
                if parse_error:
                    self.assertFalse(json_matched(identity, go, rust, Path(".")))
                else:
                    self.assertFalse(frozen_matched(go, rust, Path("."), True))
                self.assertTrue(compare_pair(row, Path(".")))
                self.assertEqual((go, rust), original)
                self.assertTrue(row["startup_classification"]["raw_observations_preserved"])
                self.assertNotIn("cache/symbrain/skills/marker.json", row["startup_classification"]["owner_bound_paths"])

    def test_unowned_files_modes_links_and_owned_extra_paths_fail(self):
        mutations = [
            ("skills-cache-bytes", lambda record: add_file(record, "cache/symbrain/skills/marker.json", b"changed")),
            ("skills-cache-mode", lambda record: record["filesystem"]["files"]["cache/symbrain/skills/marker.json"].update(mode=0o600)),
            ("new-cache-file", lambda record: add_file(record, "cache/symbrain/skills/render/hash", b"new")),
            ("unowned-link", lambda record: record["filesystem"]["files"].update({"home/unowned": {"type": "link", "target": "elsewhere"}})),
            ("extra-memory-file", lambda record: add_file(record, "data/symbrain/memory/extra.db", b"extra")),
            ("disabled-key", lambda record: add_file(record, MEMORY_KEY, b"0" * 64 + b"\n")),
            ("disabled-audit", lambda record: add_file(record, "data/symbrain/audit/skills-native-764.jsonl", b"audit")),
        ]
        for name, mutate in mutations:
            with self.subTest(name=name):
                go, rust = synthetic_pair()
                self.assertTrue(matched(go, rust, Path("."), True))
                mutate(rust)
                with self.assertRaises(AssertionError):
                    compare_pair({"id": "mcp-ordinary", "mcp": True, "go": go, "rust": rust}, Path("."))

    def test_full_protocol_and_non_skills_catalog_are_not_projected(self):
        for mutation in ("catalog", "jsonrpc", "payload-type", "stderr", "exit"):
            with self.subTest(mutation=mutation):
                go, rust = synthetic_pair()
                self.assertTrue(matched(go, rust, Path("."), True))
                responses = [json.loads(line) for line in bytes.fromhex(rust["stdout_hex"]).split(b"\n")]
                if mutation == "catalog":
                    responses[1]["result"]["tools"][0]["description"] = "changed non-Skills tool"
                elif mutation == "jsonrpc":
                    responses[1]["jsonrpc"] = "wrong"
                elif mutation == "payload-type":
                    responses[2]["result"]["content"][0]["text"] = '{"ok":1}'
                elif mutation == "stderr":
                    rust["stderr_hex"] = b"diagnostic".hex()
                else:
                    rust["exit"] = 2
                rust["stdout_hex"] = b"\n".join(json.dumps(row).encode() for row in responses).hex()
                try:
                    accepted = compare_pair({"id": "mcp-ordinary", "mcp": True, "go": go, "rust": rust}, Path("."))
                except AssertionError:
                    accepted = False
                self.assertFalse(accepted)

    def test_missing_binding_and_tampered_owned_mode_or_raw_hash_fail(self):
        for mutation in ("binding", "key-mode", "key-raw-hash"):
            with self.subTest(mutation=mutation):
                go, rust = synthetic_pair()
                if mutation == "binding":
                    rust.pop("startup_input")
                elif mutation == "key-mode":
                    go["filesystem"]["files"][MEMORY_KEY]["mode"] = 0o644
                    if os.name == "nt":
                        go["startup_privacy"]["entries"][MEMORY_KEY]["aces"][0]["sid"] = "S-1-1-0"
                else:
                    go["filesystem"]["raw_file_hashes"][MEMORY_KEY]["sha256"] = "a" * 64
                with self.assertRaises(AssertionError):
                    pair(go, rust)

    def test_input_binding_rejects_other_profiles_config_and_overrides(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "profile.toml", PROFILE)
            self.assertEqual(bind(root, {}), BINDINGS[0])
            with self.assertRaisesRegex(AssertionError, "override"):
                bind(root, {"SYMBRAIN_AUDIT_ENABLED": "true"})
            write(root / "config/symbrain/config.toml", "")
            with self.assertRaisesRegex(AssertionError, "global config"):
                bind(root, {})
            (root / "config/symbrain/config.toml").unlink()
            write(root / "profile.toml", PROFILE.replace('enabled = false', 'enabled = true', 1))
            with self.assertRaisesRegex(AssertionError, "profile"):
                bind(root, {})

    def test_cli_remains_strict(self):
        go, rust = synthetic_pair()
        go["stdout_hex"] = rust["stdout_hex"] = b"same CLI\n".hex()
        self.assertFalse(matched(go, rust, Path("."), False))

    def test_input_controls_keep_complete_bootstrap_and_catalog(self):
        go, _ = synthetic_pair()
        _, mutant = synthetic_pair(parse_error=True)
        assert_control_bootstrap(go, mutant)
        responses = [json.loads(line) for line in bytes.fromhex(mutant["stdout_hex"]).split(b"\n")]
        responses[1]["result"]["tools"][0]["description"] = "changed bootstrap"
        mutant["stdout_hex"] = b"\n".join(json.dumps(row).encode() for row in responses).hex()
        with self.assertRaisesRegex(AssertionError, "bootstrap/catalog"):
            assert_control_bootstrap(go, mutant)


class WindowsPrivacyStructureTest(unittest.TestCase):
    def test_raw_descriptor_path_and_claim_mutations_fail_entrypoint(self):
        root = Path('.')
        for mutation in ('raw-stdout', 'claims-only', 'truncated', 'nonhex', 'missing', 'path', 'deny-before-allow'):
            with self.subTest(mutation=mutation):
                go, rust = synthetic_pair()
                for record in (go, rust):
                    attach_synthetic_privacy(record)
                with patch('startup_privacy.os.name', 'nt'):
                    pair_row = {'id': 'mcp-ordinary', 'mcp': True, 'go': go, 'rust': rust}
                    self.assertTrue(compare_pair(pair_row, root))
                    proof = go['startup_privacy']
                    row = proof['entries'][MEMORY_KEY]
                    if mutation == 'raw-stdout':
                        proof['stdout_hex'] = b'{}'.hex()
                    elif mutation == 'claims-only':
                        row['owner_sid'] = 'S-1-5-18'
                    elif mutation == 'missing':
                        row.pop('descriptor_hex')
                    elif mutation in ('truncated', 'nonhex'):
                        row['descriptor_hex'] = '00' if mutation == 'truncated' else 'zz'
                    elif mutation == 'path':
                        row['path'] = 'C:\\outside\\key'
                    else:
                        row['aces'].insert(0, {'type': 1, 'flags': 0, 'mask': 1, 'sid': row['user_sid']})
                        seal_descriptor(row)
                    if mutation not in ('raw-stdout', 'claims-only'):
                        proof['stdout_hex'] = json.dumps([proof['entries'][path] for path in proof['paths']]).encode().hex()
                    with self.assertRaises((AssertionError, KeyError)):
                        compare_pair(pair_row, root)

    def acl(self):
        return synthetic_acl()

    def test_native_owner_and_dacl_policy_positive_and_negative_controls(self):
        mutations = [
            ("wrong-owner", lambda row: row.update(owner_sid="S-1-5-18")),
            ("null-dacl", lambda row: row.update(dacl_present=False)),
            ("missing-dacl-flag", lambda row: row.update(control_flags=0)),
            ("empty-dacl", lambda row: row.update(aces=[])),
            ("everyone", lambda row: row["aces"].append({"type": 0, "flags": 0, "mask": 1, "sid": "S-1-1-0"})),
            ("inherited-users", lambda row: row["aces"].append({"type": 0, "flags": 16, "mask": 1, "sid": "S-1-5-32-545"})),
            ("inherit-only-users", lambda row: row["aces"].append({"type": 0, "flags": 8, "mask": 1, "sid": "S-1-5-32-545"})),
            ("unknown-ace", lambda row: row["aces"][0].update(type=9)),
            ("owner-denied-only", lambda row: row["aces"][0].update(type=1)),
            ("owner-deny-before-allow", lambda row: row["aces"].insert(0, {"type": 1, "flags": 0, "mask": 1, "sid": row['user_sid']})),
            ("owner-inherit-only", lambda row: row["aces"][0].update(flags=8)),
        ]
        for name, mutation in mutations:
            with self.subTest(name=name):
                row = self.acl()
                validate_windows(row)
                mutation(row)
                seal_descriptor(row)
                with self.assertRaises(AssertionError):
                    validate_windows(row)

    def test_windows_requires_native_acl_even_when_posix_mode_looks_private(self):
        record: dict = {"filesystem": {"files": {"owned": {"type": "file", "mode": 0o600}}}}
        with patch("startup_privacy.os.name", "nt"):
            with self.assertRaisesRegex(AssertionError, "native ACL"):
                private(record, "owned")
            attach_synthetic_privacy(record)
            private(record, "owned")
            record["startup_privacy"]["entries"]["owned"]["owner_sid"] = "S-1-5-18"
            with self.assertRaisesRegex(AssertionError, "raw ACL/entry"):
                private(record, "owned")


if __name__ == "__main__":
    unittest.main()
