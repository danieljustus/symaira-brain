#!/usr/bin/env python3
"""Capture a clean immutable production Go process; retain raw output on failure."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args]).decode().strip()


def capture(revision, output):
    if not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise ValueError('require full lowercase immutable commit')
    root = Path(__file__).resolve().parents[2]
    if git(root, 'status', '--porcelain=v1', '--untracked-files=all'):
        raise ValueError('capture requires clean caller checkout')
    if git(root, 'rev-parse', revision + '^{commit}') != revision:
        raise ValueError('source revision mismatch')
    subprocess.run(['git', '-C', str(root), 'merge-base', '--is-ancestor', revision, 'HEAD'], check=True)
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    build = Path('/Volumes/1TB_NVMe_SN850X/Dev/builds/symaira-brain/openusage-meters/oracle-v1') / output.name
    build.mkdir(parents=True, exist_ok=False)
    source = build / 'source'
    subprocess.run(['git', '-C', str(root), 'worktree', 'add', '--detach', str(source), revision], check=True)
    assert git(source, 'status', '--porcelain=v1', '--untracked-files=all') == ''
    assert git(source, 'rev-parse', 'HEAD') == revision
    launcher = '/Users/daniel/.local/bin/dev-external'
    binary = build / 'oracle'
    env = os.environ.copy()
    env.update(CGO_ENABLED='0', GOTOOLCHAIN='go1.26.7')
    commands = []

    def run(name, args, stdin=None):
        with (output / (name + '.stdout')).open('wb') as stdout, (output / (name + '.stderr')).open('wb') as stderr:
            proc = subprocess.run(args, cwd=root, env=env, input=stdin, stdout=stdout, stderr=stderr, timeout=240)
        commands.append(dict(name=name, command=args, exit_code=proc.returncode))
        (output / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        proc.check_returncode()

    run('guard', [launcher, '--status'])
    guard = json.loads((output / 'guard.stdout').read_bytes())
    assert guard['healthy'] and guard['space']['external']['available_bytes'] >= 50 * 1024**3
    run('build', [launcher, 'go', '-C', str(source), 'build', '-o', str(binary), './scripts/openrouter-oracle-v1'])
    inputs = (source / 'scripts/openrouter-oracle-v1/inputs.json').read_bytes()
    run('go', [str(binary)], inputs)
    run('go-repeat', [str(binary)], inputs)
    assert (output / 'go.stdout').read_bytes() == (output / 'go-repeat.stdout').read_bytes()
    assert git(source, 'status', '--porcelain=v1', '--untracked-files=all') == ''
    assert git(source, 'rev-parse', 'HEAD') == revision
    artifact = json.loads((output / 'go.stdout').read_bytes())
    assert [c['input'] for c in artifact['cases']] == json.loads(inputs)
    digests = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in output.iterdir() if p.is_file()}
    receipt = dict(schema_version=1, source_commit=revision, source_tree=git(source, 'rev-parse', 'HEAD^{tree}'),
                   source_checkout=str(source), clean_before=True, clean_after=True,
                   binary=str(binary), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                   inputs_sha256=hashlib.sha256(inputs).hexdigest(), artifacts=digests,
                   case_count=len(artifact['cases']), timestamp_normalization='snapshot.fetched_at = Unix epoch',
                   source_class='offline production Go process with scripted HTTP transport; no live endpoints')
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    capture(*sys.argv[1:])
