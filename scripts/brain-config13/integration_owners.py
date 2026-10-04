"""Exact normal-main source overlay; no runtime acceptance or owner selection."""
import hashlib
import json
from pathlib import Path
import subprocess

BRAIN = 'e30676fdaced483a3cd1588a78253a3f15096d04'
MAIN = '9353520a34c5819c7d0a6cd58d3bfaa2b22b045b'
MANIFEST = 'migration/evidence/brain-config13/main935-integration/effective-owners.json'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def check_identity(name, data, mode, expected):
    assert digest(data) == expected['sha256'], (name, 'body differs')
    assert mode == expected['mode'], (name, 'mode differs')


def check_coverage(expected, actual):
    assert set(expected) == set(actual), 'missing or foreign effective owner'


def effective_owners(root, head):
    def git(*args):
        return subprocess.check_output(['git', *args], cwd=root)
    manifest = json.loads((root / MANIFEST).read_bytes())
    assert manifest['brain_parent'] == BRAIN and manifest['main_parent'] == MAIN
    for parent in (BRAIN, MAIN):
        subprocess.run(['git', 'merge-base', '--is-ancestor', parent, head], cwd=root, check=True)
    def tree(revision):
        records = {}
        for row in git('ls-tree', '-rz', revision).split(b'\0'):
            if not row:
                continue
            metadata, path = row.split(b'\t')
            mode, kind, oid = metadata.decode().split()
            if kind == 'blob':
                records[path.decode()] = (mode, oid)
        return records
    current, historical, intended = tree(head), tree(BRAIN), tree(MAIN)
    relevant = lambda name: name.startswith(('rust/', 'scripts/')) or name in ('Cargo.toml', 'Cargo.lock')
    check_coverage(manifest['effective_source'], (name for name in current if relevant(name)))
    for name, expected in manifest['effective_source'].items():
        mode, oid = current[name]
        check_identity(name, git('cat-file', 'blob', oid), mode, expected)
        actual = root / name
        check_identity(name, actual.read_bytes(), '100755' if actual.stat().st_mode & 0o111 else '100644', expected)
    for name, owner in manifest['changed_owners'].items():
        old = owner['historical']
        if old is None:
            assert name not in historical, (name, 'unexpected historical owner')
        else:
            mode, oid = historical[name]
            check_identity(name, git('cat-file', 'blob', oid), mode, old)
        if owner['owner'] == 'published-main':
            mode, oid = intended[name]
            check_identity(name, git('cat-file', 'blob', oid), mode, manifest['effective_source'][name])
        else:
            assert owner['owner'] == 'additive-source-preflight'
            assert name in ('scripts/brain-config13/runtime_tools.py', 'scripts/brain-config13/integration_owners.py', 'scripts/brain-config13/integration_owners_tests.py')
    return manifest


def owner_hash(manifest, name, historical_expected):
    if name not in manifest['changed_owners']:
        return historical_expected
    previous = manifest['changed_owners'][name]['historical']
    assert previous is not None and previous['sha256'] == historical_expected, (name, 'historical expectation differs')
    return manifest['effective_source'][name]['sha256']
