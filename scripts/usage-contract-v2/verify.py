#!/usr/bin/env python3
"""Versioned retained-evidence approval, never a fallback to the older capture."""
import json
from pathlib import Path
import re
import stat
import sys
from capture import digest, git, histories, inventory, named, require_clean, OPENROUTER_TEST


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def artifact(root, name, expected):
    require(isinstance(name, str) and not Path(name).is_absolute(), 'unsafe artifact path')
    p = root / name
    require('..' not in Path(name).parts and p.resolve().is_relative_to(root.resolve()), 'artifact path escape')
    require(stat.S_ISREG(p.lstat().st_mode), 'artifact must be regular')
    raw = p.read_bytes()
    require(type(expected['bytes']) is int and len(raw) == expected['bytes'], 'artifact size mismatch')
    require(digest(raw) == expected['sha256'], 'artifact hash mismatch')
    return raw


def anchor_for(evidence, receipt_sha256):
    """Bind a retained receipt to the hash the reviewer recorded out of band."""
    require(isinstance(receipt_sha256, str) and re.fullmatch('[0-9a-f]{64}', receipt_sha256), 'invalid receipt hash')
    receipt = json.loads((evidence / 'receipt.json').read_bytes())
    return dict(source_commit=receipt['source_commit'], receipt_sha256=receipt_sha256, inputs=receipt['inputs'])


def validate(root, evidence, anchor):
    source_commit = anchor['source_commit']
    require(isinstance(source_commit, str) and re.fullmatch('[0-9a-f]{40}', source_commit), 'invalid source commit')
    raw = (evidence / 'receipt.json').read_bytes()
    require(digest(raw) == anchor['receipt_sha256'], 'receipt anchor mismatch')
    receipt = json.loads(raw)
    require(receipt['source_commit'] == source_commit, 'source identity mismatch')
    source = Path(receipt['source_checkout'])
    require_clean(source, source_commit)
    require_clean(root, git(root, 'rev-parse', 'HEAD'))
    require(git(source, 'rev-parse', 'HEAD^{tree}') == receipt['source_tree'], 'source tree mismatch')
    require(git(root, 'merge-base', source_commit, 'HEAD') == source_commit, 'capture source not ancestor')
    require(receipt['clean_before'] is True and receipt['clean_after'] is True, 'unclean capture')
    require(inventory(source) == inventory(root) == receipt['inputs'] == anchor['inputs'], 'current source freshness mismatch')
    for name, expected in receipt['artifacts'].items():
        artifact(evidence, name, expected)
    for item in receipt['binaries'].values():
        p = Path(item['path'])
        require(stat.S_ISREG(p.lstat().st_mode), 'binary must be regular')
        require(digest(p.read_bytes()) == item['sha256'], 'binary mismatch')
    require(set(receipt['binaries']) == {'go_oracle', 'opencode_oracle', 'go_cli', 'rust_cli'}, 'binary inventory mismatch')
    commands = json.loads((evidence / 'commands.json').read_bytes())
    require([c['name'] for c in commands] == ['guard', 'build-openrouter', 'openrouter-go', 'openrouter-repeat', 'openrouter-rust', 'build-opencode', 'opencode-go', 'history-go', 'history-rust', 'build-go-cli', 'build-rust-cli', 'cli-smoke'], 'command inventory mismatch')
    require(all(type(c['exit_code']) is int and c['exit_code'] == 0 for c in commands), 'failed child command')
    go_bytes = (evidence / 'openrouter-go.stdout').read_bytes()
    require(go_bytes == (evidence / 'openrouter-repeat.stdout').read_bytes(), 'nondeterministic OpenRouter')
    go = json.loads(go_bytes)
    require([c['input'] for c in go['cases']] == json.loads((source / 'scripts/openrouter-oracle-v1/inputs.json').read_bytes()), 'OpenRouter input mismatch')
    require(len(go['cases']) == receipt['openrouter_cases'] == 27, 'OpenRouter count mismatch')
    named((evidence / 'openrouter-rust.stdout').read_text(), OPENROUTER_TEST)
    require(json.loads((evidence / 'openrouter-rust.json').read_bytes()) == go['cases'], 'complete OpenRouter reports differ')
    old = json.loads((source / 'rust/symbrain-usage/tests/fixtures/opencode_discovery_go_dependency_update_20261008.json').read_bytes())
    require(json.loads((evidence / 'opencode-output.json').read_bytes())['cases'] == old['cases'], 'legacy OpenCode cases differ')
    require(histories((evidence / 'history-go.stdout').read_text()) == histories((evidence / 'history-rust.stdout').read_text()) == json.loads((evidence / 'history-paired.json').read_bytes()), 'history differential mismatch')
    named((evidence / 'history-rust.stdout').read_text(), 'grok_history_paired_cases')
    smoke = json.loads((evidence / 'smoke/cli-smoke.json').read_bytes())
    expected_cases = {'missing-auth', 'expired-auth', 'api-network-denied', 'disabled', 'unselected', 'invalid-selector', 'no-data', 'true-zero', 'missing-cost', 'conflict', 'tail', 'symlink', 'fifo', 'table'}
    require({(r['language'], r['case']) for r in smoke} == {(lang, case) for lang in ('go', 'rust') for case in expected_cases} and len(smoke) == 28, 'CLI inventory mismatch')
    require(all(r['success'] is True and type(r['exit_code']) is int and r['exit_code'] == (2 if r['case'] == 'invalid-selector' else 0) for r in smoke), 'CLI failed assertion')
    for row in smoke:
        stem = row['language'] + '-' + row['case']
        observed = json.loads((evidence / ('smoke/' + stem + '.inventory.json')).read_bytes())
        require(observed['before'] == observed['after'], 'CLI modified protected input')
    for case in expected_cases - {'invalid-selector', 'table'}:
        require(json.loads((evidence / ('smoke/go-' + case + '.stdout')).read_bytes()) == json.loads((evidence / ('smoke/rust-' + case + '.stdout')).read_bytes()), 'complete CLI reports differ')
    return receipt


if __name__ == '__main__':
    root, evidence = (Path(p).resolve() for p in sys.argv[1:3])
    r = validate(root, evidence, anchor_for(evidence, sys.argv[3]))
    print(json.dumps(dict(verdict='PASS', source_commit=r['source_commit'], acceptance_head=git(root, 'rev-parse', 'HEAD'), openrouter_cases=27, history_cases=17, cli_cases=28)))
