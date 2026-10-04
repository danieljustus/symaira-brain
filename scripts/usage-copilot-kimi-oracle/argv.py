"""Actual frozen-Go/reviewed-parent/candidate argv diagnostics; no provider HTTP."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


# These eleven byte strings are unchanged from the independent22-case proof.
ORIGINAL = [b'--help', b'---help', b'----help', b'----json', b'----not-a-usage-flag',
            b'--owned-\xff', b'--owned-\xe2\x82', b'owned-\xff',
            b'--not-a-usage-flag=\xff', b'--not-a-usage-flag', b'ordinary-argument']
EXPANDED = [[b'-'], [b''], [b'--', b'--help'], [b'---', b'--help'],
            [b'--', b'---help'], [b'--', b'--owned-\xff'], [b'--', b'owned-\xe2\x82'],
            [b'first', b'----help'], [b'first', b'--help'], [b'--help', b'first'],
            [b'--not-a-usage-flag', b'--help'], [b'-h=false', b'--unknown'],
            [b'--help=\xff', b'first'], [b'--=name'], [b'---=name'], [b'----\xe2\x82'],
            [b'--unknown=a=b'], [b'--helpful'], [b'--owned-\xc0\xaf'], [b'--owned-\xed\xa0\x80'],
            [b'--owned-\xf4\x90\x80\x80'], [b'--owned-\x80'], [b'--owned-\n'],
            [b'owned-\xe2\x82'], [b'owned-\x01'], [b'owned-\n'], [b'owned-"\\'],
            ['owned-é'.encode()], ['owned-🙂'.encode()], ['owned-\u00a0'.encode()]]


def supported(arguments):
    if os.name != 'nt':
        return True
    try:
        for argument in arguments:
            argument.decode('utf8')
        return True
    except UnicodeDecodeError:
        return False


def observe(binary, arguments, environment, home):
    command = [str(binary.resolve()), 'usage', *[os.fsdecode(value) for value in arguments]]
    if binary.suffix == '.py':
        command.insert(0, sys.executable)
    process = subprocess.run(command, cwd=home, env=environment, capture_output=True, timeout=15)
    return dict(exit=process.returncode, stdout_b64=base64.b64encode(process.stdout).decode(),
                stderr_b64=base64.b64encode(process.stderr).decode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['go', 'parent', 'rust', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--parent-source', required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    original_receipt = repo / 'migration/evidence/usage-argv-768/original-abf-review/proof/symaira-usage768-abf-root-doctor-cli-extra.json'
    historical = json.loads(original_receipt.read_text())
    assert historical['cases'] == 22 and historical['passed'] == 10 and historical['failed'] == 12
    for provider in ['copilot', 'kimi']:
        assert [row['arg_hex'] for row in historical['rows'] if row['provider'] == provider] == [value.hex() for value in ORIGINAL]
    retained = {(row['provider'], row['arg_hex']): row for row in historical['rows']}
    records, skipped = [], []
    vectors = [('original-' + str(index), [flag], True) for index, flag in enumerate(ORIGINAL)]
    vectors += [('expanded-' + str(index), values, False) for index, values in enumerate(EXPANDED)]
    with tempfile.TemporaryDirectory(prefix='usage768-owned-argv-') as temporary:
        for provider in ['copilot', 'kimi']:
            home = Path(temporary) / provider
            home.mkdir()
            path = home / ('.config/github-copilot/apps.json' if provider == 'copilot'
                           else '.kimi-code/credentials/kimi-code.json')
            path.parent.mkdir(parents=True)
            data = (b'{"github.com:a":{"oauth_token":"env://OWNED_LITERAL"}}' if provider == 'copilot'
                    else b'{"access_token":"env://OWNED_LITERAL"}')
            path.write_bytes(data)
            before = sorted(str(p.relative_to(home)) for p in home.rglob('*'))
            environment = {key: value for key, value in os.environ.items()
                           if key in ['SystemRoot', 'SYSTEMROOT', 'WINDIR', 'TMP', 'TEMP', 'PATHEXT']}
            environment.update(HOME=str(home), USERPROFILE=str(home), PATH='',
                               XDG_CONFIG_HOME=str(home / 'config'), XDG_CACHE_HOME=str(home / 'cache'),
                               XDG_DATA_HOME=str(home / 'data'), ANTHROPIC_OAUTH_TOKEN='env://ABSENT',
                               SYMBRAIN_GO_BINARY=str(home / 'absent-fallback'))
            for name, values, original in vectors:
                case = dict(id=provider + '-' + name, provider=provider, argv_hex=[value.hex() for value in values], original=original)
                if not supported(values):
                    skipped.append(dict(case, reason='Raw invalid Unix UTF8 argv cannot be launched through Python Windows argument strings; no Windows surrogate proof'))
                    continue
                go, parent, rust = [observe(binary, values, environment, home)
                                    for binary in [args.go, args.parent, args.rust]]
                row = dict(case, go=go, parent=parent, rust=rust, matched=go == rust,
                           parent_matched=go == parent, file_sha256=hashlib.sha256(data).hexdigest(), read_only=True)
                if original:
                    old = retained[(provider, values[0].hex())]
                    assert old['go'] == go == old['parent'] and old['native'] == parent, row
                    assert old['file_sha256'] == row['file_sha256'], row
                records.append(row)
                assert path.read_bytes() == data and sorted(str(p.relative_to(home)) for p in home.rglob('*')) == before, row
                assert go['exit'] == parent['exit'] == rust['exit'] == 2 and not any(
                    observation['stdout_b64'] for observation in [go, parent, rust]), row
                assert row['matched'], row
    original = [row for row in records if row['original']]
    assert len(original) == (14 if os.name == 'nt' else 22)
    assert sum(not row['parent_matched'] for row in original) == (6 if os.name == 'nt' else 12)
    source_files = ['rust/symbrain-cli/src/usage_cli.rs', 'rust/symbrain-cli/src/lib.rs',
                    'rust/symbrain-core/src/config/format.rs', 'rust/symbrain-core/src/config/set.rs',
                    'rust/symbrain-core/src/config/mod.rs', 'rust/symbrain-core/src/go_printable.rs',
                    'scripts/usage-copilot-kimi-oracle/argv.py']
    go_root = Path(subprocess.check_output(['go', 'env', 'GOROOT'], text=True).strip())
    sdk_files = ['src/flag/flag.go', 'src/strconv/quote.go']
    receipt = dict(original_review_sha256=hashlib.sha256(original_receipt.read_bytes()).hexdigest(),
                   go_sdk_source_sha256={name: hashlib.sha256((go_root / name).read_bytes()).hexdigest() for name in sdk_files},
                   candidate_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
                   candidate_dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=repo)),
                   parent_source=args.parent_source, cases=len(records), passed=len(records), failed=0,
                   original22_inputs_sha256=hashlib.sha256(json.dumps([value.hex() for value in ORIGINAL]).encode()).hexdigest(),
                   original_cases=len(original), original_parent_mismatches=sum(not row['parent_matched'] for row in original),
                   records=records, platform_skips=skipped, fallback='Absent for actual parent and candidate; direct frozenGo CLI is the oracle',
                   source_sha256={name: hashlib.sha256((repo / name).read_bytes()).hexdigest() for name in source_files},
                   binary_sha256={name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in [('go', args.go), ('parent', args.parent), ('rust', args.rust)]},
                   scope='All executed arguments are invalid or help and return2 before service report/HTTP; owned literal files remain byte/tree read-only. All original22 inputs retained, Windows invalid Unix byte cases explicitly unexecuted.')
    args.output.write_text(json.dumps(receipt, indent=2) + '\n')
    print('Usage argv:', len(records), 'exact candidate pairs;', len(original), 'original inputs;', receipt['original_parent_mismatches'], 'retained parent failures')


if __name__ == '__main__':
    main()
