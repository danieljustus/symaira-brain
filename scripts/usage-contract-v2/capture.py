#!/usr/bin/env python3
"""Capture the combined additive usage contract from a clean immutable checkout."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

LAUNCHER = '/Users/daniel/.local/bin/dev-external'
ROOT = Path(__file__).resolve().parents[2]
HISTORICAL = 'b807eae643c627ae31166b90176d5a0de6f4e647'
OPENROUTER_TEST = 'providers::openrouter_go_oracle_tests::current_openrouter_v1_complete_reports_and_requests_match_executed_go'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args]).decode().strip()


def inventory(root):
    paths = git(root, 'ls-files').splitlines()
    return {p: digest((root / p).read_bytes()) for p in paths
            if p != 'scripts/usage-contract-v2/accepted.json'}


def require_clean(root, revision):
    assert re.fullmatch('[0-9a-f]{40}', revision), 'full immutable commit required'
    assert git(root, 'rev-parse', 'HEAD') == revision, 'checkout identity mismatch'
    assert not git(root, 'status', '--porcelain=v1', '--untracked-files=all'), 'dirty source'


def named(stdout, name, count=1):
    assert f'test {name} ... ok' in stdout, 'named Rust test did not execute'
    assert re.search(rf'test result: ok\. {count} passed; 0 failed; 0 ignored;', stdout), 'Rust execution count mismatch'


def histories(stdout):
    result = {}
    for line in stdout.splitlines():
        if line.startswith('HISTORY_CASE '):
            _, name, encoded = line.split(' ', 2)
            assert name not in result
            result[name] = json.loads(encoded)
    assert len(result) == 17, 'history execution inventory mismatch'
    return result


def main(revision, evidence, build):
    require_clean(ROOT, git(ROOT, 'rev-parse', 'HEAD'))
    assert re.fullmatch('[0-9a-f]{40}', revision)
    assert git(ROOT, 'rev-parse', revision + '^{commit}') == revision
    evidence, build = Path(evidence).resolve(), Path(build).resolve()
    assert build.is_relative_to(Path('/Volumes/1TB_NVMe_SN850X/Dev/builds/symaira-brain'))
    evidence.mkdir(parents=True, exist_ok=False)
    build.mkdir(parents=True, exist_ok=False)
    source = build / 'source'
    subprocess.run(['git', '-C', str(ROOT), 'worktree', 'add', '--detach', str(source), revision], check=True)
    require_clean(source, revision)
    before = inventory(source)
    (evidence / 'inputs-before.json').write_text(json.dumps(before, indent=2) + '\n')
    env = os.environ.copy()
    env.update(CGO_ENABLED='0', GOPROXY='off', GOTOOLCHAIN='go1.26.7',
               CARGO_TARGET_DIR=str(build / 'cargo'))
    sandbox = build / 'test-home'
    sandbox.mkdir()
    runner = source / 'scripts/usage-contract-v2/test-runner.py'
    env.update(USAGE_V2_TEST_HOME=str(sandbox), CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER=str(runner))
    commands = []

    def run(name, args, stdin=None, cwd=source):
        if name != 'guard':
            guard = subprocess.run([LAUNCHER, '--status'], capture_output=True, check=True)
            (evidence / (name + '.guard.json')).write_bytes(guard.stdout)
            g = json.loads(guard.stdout)
            assert g['healthy'] and g['space']['external']['available_bytes'] >= 5 * 1024**3
        with (evidence / (name + '.stdout')).open('wb') as out, (evidence / (name + '.stderr')).open('wb') as err:
            p = subprocess.run(args, cwd=cwd, env=env, input=stdin, stdout=out, stderr=err, timeout=1200)
        commands.append(dict(name=name, command=args, exit_code=p.returncode))
        (evidence / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        p.check_returncode()
        return (evidence / (name + '.stdout')).read_bytes()

    run('guard', [LAUNCHER, '--status'])
    run('build-openrouter', [LAUNCHER, 'go', 'build', '-o', str(build / 'openrouter'), './scripts/openrouter-oracle-v1'])
    inputs = (source / 'scripts/openrouter-oracle-v1/inputs.json').read_bytes()
    go = run('openrouter-go', [str(build / 'openrouter')], inputs)
    assert go == run('openrouter-repeat', [str(build / 'openrouter')], inputs)
    env.update(OPENROUTER_GO_V1=str(evidence / 'openrouter-go.stdout'), OPENROUTER_NATIVE_V1=str(evidence / 'openrouter-rust.json'))
    rust = run('openrouter-rust', [LAUNCHER, 'cargo', 'test', '--locked', '--offline', '-p', 'symbrain-usage', '--lib', OPENROUTER_TEST, '--', '--exact', '--nocapture'])
    named(rust.decode(), OPENROUTER_TEST)
    assert json.loads((evidence / 'openrouter-rust.json').read_bytes()) == json.loads(go)['cases']
    for name in ('OPENROUTER_GO_V1', 'OPENROUTER_NATIVE_V1'):
        env.pop(name)
    # Execute the existing generator without check mode; never rewrite frozen fixtures.
    run('build-opencode', [LAUNCHER, 'go', 'build', '-o', str(build / 'opencode'), './scripts/usage-request-oracle/opencode'])
    run('opencode-go', [str(build / 'opencode'), '-oracle-revision', revision, '-oracle-toolchain', 'go1.26.7', '-output', str(evidence / 'opencode-output.json')])
    opencode = (evidence / 'opencode-output.json').read_bytes()
    old = json.loads((source / 'rust/symbrain-usage/tests/fixtures/opencode_discovery_go_dependency_update_20261008.json').read_bytes())
    assert json.loads(opencode)['cases'] == old['cases'], 'legacy OpenCode behavior changed'
    go_history = run('history-go', [LAUNCHER, 'go', 'test', '-exec', str(runner), './internal/usage', '-run', '^TestGrokHistoryCases$', '-count=1', '-v'])
    rust_history = run('history-rust', [LAUNCHER, 'cargo', 'test', '--locked', '--offline', '-p', 'symbrain-usage', '--test', 'history_tests', 'grok_history_paired_cases', '--', '--exact', '--nocapture'])
    named(rust_history.decode(), 'grok_history_paired_cases')
    assert histories(go_history.decode()) == histories(rust_history.decode())
    (evidence / 'history-paired.json').write_text(json.dumps(histories(go_history.decode()), indent=2) + '\n')
    run('build-go-cli', [LAUNCHER, 'go', 'build', '-o', str(build / 'symbrain-go'), './cmd/symbrain'])
    run('build-rust-cli', [LAUNCHER, 'cargo', 'build', '--locked', '--offline', '-p', 'symbrain-cli', '--bin', 'symbrain'])
    run('cli-smoke', [sys.executable, str(source / 'scripts/usage-contract-v2/cli-smoke.py'), str(evidence / 'smoke'), str(build), env['CARGO_TARGET_DIR']])
    require_clean(source, revision)
    after = inventory(source)
    assert before == after, 'executed source changed'
    (evidence / 'inputs-after.json').write_text(json.dumps(after, indent=2) + '\n')
    binaries = {name: dict(path=str(path), sha256=digest(path.read_bytes())) for name, path in {
        'go_oracle': build / 'openrouter', 'opencode_oracle': build / 'opencode', 'go_cli': build / 'symbrain-go',
        'rust_cli': Path(env['CARGO_TARGET_DIR']) / 'debug/symbrain'}.items()}
    artifacts = {str(p.relative_to(evidence)): dict(sha256=digest(p.read_bytes()), bytes=p.stat().st_size)
                 for p in sorted(evidence.rglob('*')) if p.is_file()}
    receipt = dict(schema_version=2, source_commit=revision, source_tree=git(source, 'rev-parse', 'HEAD^{tree}'),
                   source_checkout=str(source), clean_before=True, clean_after=True,
                   source_class='offline actual Go/Rust processes; synthetic inputs; no live provider evidence',
                   inputs=before, binaries=binaries, artifacts=artifacts,
                   openrouter_cases=27, history_cases=17, cli_cases=28)
    (evidence / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(dict(source_commit=revision, receipt_sha256=digest((evidence / 'receipt.json').read_bytes()), verdict='PASS')))


if __name__ == '__main__':
    main(*sys.argv[1:])
