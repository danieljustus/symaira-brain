#!/usr/bin/env python3
"""Verify isolated source correction/provenance only, never build or run it."""
import argparse
import ast
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def verify_map(root, entries):
    for name, expected in entries.items():
        if sha((root / name).read_bytes()) != expected:
            raise AssertionError(f'changed source bytes: {root / name}')
    return len(entries)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    root = args.root.resolve()
    proof = root / 'migration/evidence/memory-sync-762'
    inventory = json.loads((proof / 'backend-corrections-static/source-inventory.json').read_text())
    original = '8a3226828d045b4307d2c3601e283d1ecfa4ce4f'
    retained = json.loads((proof / 'independent-8a/retention.json').read_text())
    if retained['original_proof_directory_files'] != 61 or len(retained['entries']) != 64:
        raise AssertionError('complete original review/proof set required')
    for row in retained['entries']:
        archive = (root / row['archive']).read_bytes()
        raw = gzip.decompress(archive)
        if sha(archive) != row['archive_sha256'] or (sha(raw), len(raw)) != (row['sha256'], row['bytes']):
            raise AssertionError(f'changed original review: {row["archive"]}')
    receipt = json.loads(gzip.decompress((proof / 'independent-8a/symaira-memory762-backends-independent-static-review-receipt.json.gz').read_bytes()))
    if receipt['status'] != 'REQUEST_CHANGES_STATIC_ONLY' or len(receipt['findings']) != 3:
        raise AssertionError('do not replace original REQUEST with a subset')
    counts = {'retained_review_files': 64, 'retained_review_proof_files': 61}
    base = json.loads((proof / 'backends-static/source-inventory.json').read_text())
    counts['frozen'] = verify_map(Path(base['frozen_root']), receipt['source_maps']['frozen_sha256'])
    counts['SDK'] = verify_map(Path(base['sdk_root']), receipt['source_maps']['sdk_sha256'])
    counts['current_source'] = verify_map(root, inventory['source_sha256'])
    for name, expected in receipt['source_maps']['native_sha256'].items():
        raw = subprocess.check_output(['git', 'show', original + ':' + name], cwd=root)
        if sha(raw) != expected:
            raise AssertionError(f'original source binding changed: {name}')
        if name not in inventory['allowed_changed_original_paths'] and (root / name).read_bytes() != raw:
            raise AssertionError(f'unrelated source change: {name}')
    old_plan = root / 'scripts/memory-sync-oracle/cases.json'
    backend_plan = root / 'scripts/memory-sync-oracle/backend-cases.json'
    for path in [old_plan, backend_plan]:
        if path.read_bytes() != subprocess.check_output(['git', 'show', original + ':' + str(path.relative_to(root))], cwd=root):
            raise AssertionError('original full case/control plans changed')
    new_plan = json.loads((root / 'scripts/memory-sync-oracle/redirect-correction-cases.json').read_text())
    if sha(old_plan.read_bytes()) != new_plan['original230_case_plan_sha256'] or sha(backend_plan.read_bytes()) != new_plan['original98_backend_plan_sha256']:
        raise AssertionError('old plan digests changed')
    if len(new_plan['cases']) != 64 or len({c['id'] for c in new_plan['cases']}) != 64 or len(new_plan['controls']) != 3:
        raise AssertionError('full new constructor/control accounting required')
    if any(c['status'] != 'prepared_not_executed' for c in new_plan['cases'] + new_plan['controls']):
        raise AssertionError('prepared definitions are not runtime results')
    dep = json.loads((proof / 'backends-static/final-dependency-provenance.json').read_text())
    if sha((root / 'Cargo.lock').read_bytes()) != dep['final_lock_sha256']:
        raise AssertionError('dependency lock change is outside correction')
    counts['registry_sources'] = verify_map(Path(dep['registry_source_root']), dep['registry_source_sha256'])
    counts['registry_archives'] = verify_map(Path(dep['registry_cache_root']), dep['registry_archive_sha256'])
    for name in ['Cargo.toml', 'rust/symbrain-memory/Cargo.toml']:
        if (root / name).read_bytes() != subprocess.check_output(['git', 'show', original + ':' + name], cwd=root):
            raise AssertionError('dependency manifest changed')
        tomllib.loads((root / name).read_text())
    for name in inventory['source_sha256']:
        if name.endswith('.py'):
            ast.parse((root / name).read_text(), filename=name)
    lengths = {str(p.relative_to(root)): len(p.read_text().splitlines()) for p in (root / 'rust/symbrain-memory/src/sync').rglob('*.rs')}
    if any(length >= 400 for length in lengths.values()):
        raise AssertionError('sync source modules must stay below400 lines')
    model = (root / 'rust/symbrain-memory/src/sync/model.rs').read_text().split('pub struct RelayBlob {', 1)[1].split('\n}', 1)[0]
    if '#[serde' in model or 'pub blob_present: bool' not in model:
        raise AssertionError('manual blob presence must not register an unrelated Serde codec')
    counts.update(original_cases=230, original_controls=4, backend_cases=98,
                  backend_controls=4, new_cases=64, new_controls=3,
                  prepared_unit_functions=16, tests_or_controls_executed=0)
    print(json.dumps(dict(status='SOURCE_HASHES_STATIC_ONLY_NOT_ACCEPTANCE', **counts,
                          compilation=False, runtime=False, target_access=False, ports=0), indent=2))


if __name__ == '__main__':
    main()
