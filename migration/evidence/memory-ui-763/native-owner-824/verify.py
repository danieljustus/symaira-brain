#!/usr/bin/env python3
"""Verify retained original bytes, source bounds and actual binary archives."""
from pathlib import Path
import gzip
import hashlib
import io
import json
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[4]
EVIDENCE = Path(__file__).resolve().parent


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_sha(path):
    return digest(path.read_bytes())


def verify():
    proof = json.loads((EVIDENCE / 'validation.json').read_text())
    source = proof['validation_source']
    prod = proof['production_source']
    diff = subprocess.check_output([
        'git', 'diff', source, 'HEAD', '--', 'rust', 'Cargo.toml', 'Cargo.lock',
        '.github/workflows/memory-http-native.yml', 'scripts/memory-http-oracle',
    ], cwd=ROOT)
    assert not diff, 'Candidate source differs from validated source'
    for name, value in proof['candidate_source_sha256'].items():
        assert file_sha(ROOT / name) == value, name
    # Production equals D1; test scripts intentionally differ from D1.
    assert not subprocess.check_output([
        'git', 'diff', prod, source, '--', 'rust', 'Cargo.toml', 'Cargo.lock',
        '.github/workflows/memory-http-native.yml',
    ], cwd=ROOT)
    for item in proof['retained_original_files']:
        retained = ROOT / item['retained']
        assert file_sha(retained) == item['retained_sha256'], str(retained)
        original = gzip.decompress(retained.read_bytes())
        assert len(original) == item['bytes'] and digest(original) == item['sha256']
        assert file_sha(Path(item['original'])) == item['sha256'], item['original']
    receipt_path = Path('/workspace/oracles/symaira-memory763-final-binaries/receipt.json')
    assert file_sha(receipt_path) == proof['final_archive_receipt_sha256']
    receipt = json.loads(receipt_path.read_text())
    assert receipt['snapshot_head'] == source
    checked = set()
    for item in receipt['artifacts']:
        path = Path(item['archive'])
        if item['sha256'] not in checked:
            assert file_sha(path) == item['archive_sha256'], str(path)
            data = gzip.decompress(path.read_bytes())
            assert len(data) == item['bytes'] and digest(data) == item['sha256']
            checked.add(item['sha256'])
        assert file_sha(Path(item['path'])) == item['sha256'], item['path']
    for item in proof['ordinary_test_executables'] + proof['primary_binaries']:
        assert file_sha(Path(item['path'])) == item['sha256'], item['path']
    full = Path('/tmp/symaira-memory763-final-full')
    native = json.loads((full / 'receipt.json').read_text())
    assert native['head'] == source and not native['candidate_dirty']
    archive = subprocess.check_output(['git', 'archive', proof['frozen_revision']], cwd=ROOT)
    with tarfile.open(fileobj=io.BytesIO(archive)) as files:
        manifest = {item.name: digest(files.extractfile(item).read())
                    for item in files if item.isfile()}
    assert manifest == native['frozen_source_sha256']
    for name, value in manifest.items():
        assert file_sha(full / 'frozen-go' / name) == value, name
    for record in native['records']:
        report = full / ((record['mode'] or 'http') + '.json')
        assert file_sha(report) == record['report_sha256'], str(report)
    print(json.dumps(dict(
        validation_source=source, production_source=prod,
        retained_files=len(proof['retained_original_files']),
        source_files=len(proof['candidate_source_sha256']),
        frozen_source_files=len(manifest), ELF_paths=len(receipt['artifacts']),
        unique_ELF=len(checked), executed_test_binaries=len(proof['ordinary_test_executables']),
        status='PASS',
    )))


if __name__ == '__main__':
    verify()
