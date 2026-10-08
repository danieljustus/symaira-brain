#!/usr/bin/env python3
"""Verify the correction and every original source/proof/archive binding."""
from pathlib import Path
import gzip
import hashlib
import io
import json
import subprocess
import tarfile

ROOT=Path(__file__).resolve().parents[4]
EVIDENCE=Path(__file__).resolve().parent


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fsha(path):
    return digest(Path(path).read_bytes())


def archives(receipt):
    checked=set()
    for row in receipt['artifacts']:
        if row['sha256'] in checked:continue
        data=Path(row['archive']).read_bytes()
        assert digest(data)==row['archive_sha256'],row['archive']
        raw=gzip.decompress(data)
        assert len(raw)==row['bytes'] and digest(raw)==row['sha256'],row['archive']
        checked.add(row['sha256'])
    return len(checked)


def retention(rows,base):
    for row in rows:
        data=(base/row['retained']).read_bytes()
        assert digest(data)==row['retained_sha256'],row['retained']
        original=gzip.decompress(data)
        assert len(original)==row['bytes'] and digest(original)==row['sha256'],row['original']
        assert fsha(row['original'])==row['sha256'],row['original']


def verify():
    proof=json.loads((EVIDENCE/'validation.json').read_text())
    source=proof['validation_source']
    assert not subprocess.check_output(['git','diff',source,'HEAD','--','rust','Cargo.toml','Cargo.lock','scripts/memory-http-oracle','scripts/memory-cli-oracle','.github/workflows'],cwd=ROOT)
    for name,value in proof['candidate_source_sha256'].items():assert fsha(ROOT/name)==value,name
    retention(proof['retained_original_files'],ROOT)
    metadata=proof['metadata_only_enumeration_correction']
    assert fsha(EVIDENCE/metadata['original'])==metadata['sha256']
    receipt_path=Path(proof['binary_archive_receipt'])
    assert fsha(receipt_path)==proof['binary_archive_receipt_sha256']
    current=json.loads(receipt_path.read_text());assert current['source']==source
    unique=archives(current)
    assert unique==proof['archived_unique_executable_bytes']
    for row in proof['ordinary_executables']+proof['primary_executables']:
        assert fsha(row['path'])==row['sha256'],row['path']
    # Old target paths were deliberately recycled only after their verified
    # pre-build snapshot. Validate original archived bytes, never misattribute
    # current executables to the source824 receipt.
    oldroot=ROOT/'migration/evidence/memory-ui-763/independent-review-824'
    old=json.loads((oldroot/'preservation.json').read_text())
    retention(old['retained_files'],oldroot)
    original_receipt=oldroot/'original-binary-archive-receipt.json'
    assert fsha(original_receipt)==old['original_binary_archive_receipt_sha256']
    original=json.loads(original_receipt.read_text());oldunique=archives(original)
    assert len(original['artifacts'])==old['actual_ELF_paths']==526 and oldunique==old['unique_ELF_verified']==492
    baseline=json.loads((ROOT/'migration/evidence/memory-ui-763/native-owner-824/validation.json').read_text())
    retention(baseline['retained_original_files'],ROOT)
    for name,value in baseline['candidate_source_sha256'].items():
        content=subprocess.check_output(['git','show',baseline['validation_source']+':'+name],cwd=ROOT)
        assert digest(content)==value,name
    assert len(baseline['ordinary_test_executables'])==33
    archived={row['sha256'] for row in original['artifacts']}
    assert all(row['sha256'] in archived for row in baseline['ordinary_test_executables']+baseline['primary_binaries'])
    archive=subprocess.check_output(['git','archive',proof['frozen_revision']],cwd=ROOT)
    with tarfile.open(fileobj=io.BytesIO(archive)) as files:
        manifest={item.name:digest(files.extractfile(item).read()) for item in files if item.isfile()}
    assert manifest==proof['frozen_source_sha256'] and len(manifest)==2438
    full=Path('/tmp/symaira-memory763-authority-final-http')
    native=json.loads((full/'receipt.json').read_text());assert native['head']==source and not native['candidate_dirty']
    assert native['frozen_source_sha256']==manifest
    for name,value in manifest.items():assert fsha(full/'frozen-go'/name)==value,name
    for row in native['records']:
        report=full/((row['mode'] or 'http')+'.json')
        assert fsha(report)==row['report_sha256'],str(report)
    if 'verification_log_sha256' in proof:
        assert fsha(EVIDENCE/'verification.log')==proof['verification_log_sha256']
    print(json.dumps(dict(status='PASS',validation_source=source,candidate_sources=len(proof['candidate_source_sha256']),frozen_source_files=len(manifest),new_original_proofs=len(proof['retained_original_files']),original_independent_files=len(old['retained_files']),original_author_files=len(baseline['retained_original_files']),original_ELF_paths=len(original['artifacts']),original_unique_ELF=oldunique,current_actual_executable_bindings=len(current['artifacts']),current_unique_ELF=unique)))


if __name__=='__main__':verify()
