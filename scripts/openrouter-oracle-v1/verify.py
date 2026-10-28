#!/usr/bin/env python3
"""Validate retained v1 source/binary/artifact provenance and run the exact Rust gate."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

SOURCE = 'f0fb64902ba55cc3bc046af992df9ee2e81ebfb6'
CAPTURE_SHA256 = 'cfe8edcb7f1e3facc8fed10801e92e0ad6fee8177d914e5268bd47931b24dbb1'
RECEIPT_SHA256 = '72207d9868b7e1cfd341c46c78b2e5af026cd1f3e203bcc156a2818c794a1fef'
TEST = 'providers::openrouter_go_oracle_tests::current_openrouter_v1_complete_reports_and_requests_match_executed_go'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args]).decode().strip()


def validate(root, evidence, fixture):
    raw = (evidence / 'receipt.json').read_bytes()
    require(digest(raw) == RECEIPT_SHA256, 'receipt anchor mismatch')
    receipt = json.loads(raw)
    require(isinstance(receipt['source_commit'], str) and re.fullmatch(r'[0-9a-f]{40}', receipt['source_commit']), 'invalid source identity')
    require(receipt['source_commit'] == SOURCE, 'wrong captured source')
    source = Path(receipt['source_checkout'])
    require(git(source, 'rev-parse', 'HEAD') == SOURCE, 'checkout identity mismatch')
    require(git(source, 'status', '--porcelain=v1', '--untracked-files=all') == '', 'dirty captured checkout')
    require(git(source, 'rev-parse', 'HEAD^{tree}') == receipt['source_tree'], 'tree mismatch')
    subprocess.run(['git', '-C', str(root), 'merge-base', '--is-ancestor', SOURCE, 'HEAD'], check=True)
    require(git(root, 'diff', SOURCE, '--', '*.go', 'go.mod', 'go.sum') == '', 'current Go inputs differ from captured source')
    require(receipt['clean_before'] is True and receipt['clean_after'] is True, 'unclean observation')
    require(digest(Path(receipt['binary']).read_bytes()) == receipt['binary_sha256'], 'binary mismatch')
    for name, expected in receipt['artifacts'].items():
        path = (evidence / name).resolve()
        require(path.is_relative_to(evidence.resolve()) and path.is_file(), 'artifact path escape')
        require(digest(path.read_bytes()) == expected, 'artifact mismatch: ' + name)
    captured = (evidence / 'go.stdout').read_bytes()
    require(digest(captured) == CAPTURE_SHA256, 'capture anchor mismatch')
    require(fixture.read_bytes() == captured, 'fixture differs from actual Go output')
    require((evidence / 'go-repeat.stdout').read_bytes() == captured, 'nondeterministic output')
    inputs = (source / 'scripts/openrouter-oracle-v1/inputs.json').read_bytes()
    require(digest(inputs) == receipt['inputs_sha256'], 'input mismatch')
    cases = json.loads(captured)['cases']
    require([case['input'] for case in cases] == json.loads(inputs), 'input inventory mismatch')
    require(len(cases) == receipt['case_count'] == 27, 'case count mismatch')
    require(len({case['input']['id'] for case in cases}) == 27, 'duplicate cases')
    commands = json.loads((evidence / 'commands.json').read_bytes())
    require([step['name'] for step in commands] == ['guard', 'build', 'go', 'go-repeat'], 'command inventory mismatch')
    require(all(step['exit_code'] == 0 for step in commands), 'failed child command')
    require(all(not (evidence / (name + '.stderr')).read_bytes() for name in ['build', 'go', 'go-repeat']), 'unexpected diagnostics')
    return receipt


def executed(stdout, returncode):
    require(returncode == 0, 'Rust child failed')
    require('test ' + TEST + ' ... ok' in stdout, 'named Rust test did not pass')
    require(re.search(r'test result: ok\. 1 passed; 0 failed; 0 ignored;', stdout), 'Rust execution count mismatch')


def main():
    root = Path(__file__).resolve().parents[2]
    evidence = Path(sys.argv[1]).resolve()
    fixture = root / 'rust/symbrain-usage/tests/fixtures/openrouter_go_v1.json'
    receipt = validate(root, evidence, fixture)
    output = evidence.parent / sys.argv[2]
    output.mkdir(exist_ok=False)
    env = os.environ.copy()
    env.update(OPENROUTER_GO_V1=str(evidence / 'go.stdout'), OPENROUTER_NATIVE_V1=str(output / 'rust.json'),
               CARGO_TARGET_DIR='/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/builds/symaira-brain/openusage-meters/target')
    launcher = '/Users/daniel/.local/bin/dev-external'
    guard = subprocess.run([launcher, '--status'], capture_output=True, check=True)
    (output / 'guard.json').write_bytes(guard.stdout)
    require(json.loads(guard.stdout)['space']['external']['available_bytes'] >= 50 * 1024**3, 'insufficient external reserve')
    command = [launcher, 'cargo', 'test', '--locked', '-p', 'symbrain-usage', '--offline', '--lib', TEST, '--', '--exact', '--nocapture']
    proc = subprocess.run(command, cwd=root, env=env, capture_output=True, timeout=240)
    (output / 'rust.stdout').write_bytes(proc.stdout)
    (output / 'rust.stderr').write_bytes(proc.stderr)
    (output / 'command.json').write_text(json.dumps(dict(command=command, exit_code=proc.returncode), indent=2)+'\n')
    executed(proc.stdout.decode(), proc.returncode)
    require(json.loads((output / 'rust.json').read_bytes()) == json.loads((evidence / 'go.stdout').read_bytes())['cases'], 'saved differential mismatch')
    print(json.dumps(dict(source_commit=receipt['source_commit'], candidate_head=git(root, 'rev-parse', 'HEAD'), cases=27, verdict='PASS')))


if __name__ == '__main__':
    main()
