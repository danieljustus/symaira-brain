#!/usr/bin/env python3
"""Verify source/dependency provenance only; never compile or run an oracle."""
from pathlib import Path
import argparse
import hashlib
import json
import tomllib


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_map(root, entries):
    for name, expected in entries.items():
        actual = digest(root / name)
        if actual != expected:
            raise AssertionError(f"source hash mismatch: {root / name}")
    return len(entries)


def identities(lock):
    return {(p['name'], p['version'], p.get('source')): p.get('checksum')
            for p in lock['package']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    root = args.root
    proof = root / 'migration/evidence/memory-sync-762/backends-static'
    inventory = json.loads((proof / 'source-inventory.json').read_text())
    counts = {}
    for name, where in [('frozen', Path(inventory['frozen_root'])),
                        ('native', root), ('sdk', Path(inventory['sdk_root'])),
                        ('checkpoint_retained', root)]:
        counts[name] = verify_map(where, inventory[name + '_sha256'])
    old = tomllib.loads((proof / 'original-c4-Cargo.lock').read_text())
    new = tomllib.loads((root / 'Cargo.lock').read_text())
    retained, current = identities(old), identities(new)
    for name, checksum in retained.items():
        if current.get(name) != checksum or name not in current:
            raise AssertionError(f"old lock identity/checksum changed: {name}")
    dependency = json.loads((proof / 'final-dependency-provenance.json').read_text())
    if digest(root / 'Cargo.lock') != dependency['final_lock_sha256']:
        raise AssertionError('final lock bytes changed')
    verify_map(Path(dependency['registry_source_root']), dependency['registry_source_sha256'])
    verify_map(Path(dependency['registry_cache_root']), dependency['registry_archive_sha256'])
    for path, expected in dependency['resolution_logs_sha256'].items():
        if digest(proof / path) != expected:
            raise AssertionError(f'resolution log changed: {path}')
    plan = json.loads((root / 'scripts/memory-sync-oracle/backend-cases.json').read_text())
    if len(plan['cases']) != plan['additive_cases']:
        raise AssertionError('planned case accounting changed')
    if len({c['id'] for c in plan['cases']}) != len(plan['cases']):
        raise AssertionError('duplicate case ids')
    if any(c['status'] != 'prepared_not_executed' for c in plan['cases'] + plan['controls']):
        raise AssertionError('source-stage plan claims runtime result')
    if digest(root / 'scripts/memory-sync-oracle/cases.json') != plan['original_case_plan_sha256']:
        raise AssertionError('original 230-case plan changed')
    counts.update(old_locked_identities=len(retained), added_locked_identities=len(current.keys()-retained.keys()),
                  prepared_additive_cases=len(plan['cases']), prepared_additive_controls=len(plan['controls']))
    print(json.dumps(dict(status='source_hashes_and_lock_only_pass', **counts), indent=2))


if __name__ == '__main__':
    main()
