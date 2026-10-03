#!/usr/bin/env python3
"""Actual production Go/native activity validation with no fallback process."""
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
ROOT = Path(__file__).resolve().parents[2]
PROFILE = '[profile]\nname="activity-oracle"\n[servers.memory]\nenabled=true\nmode="read_only"\ntools_allow=["activity_search","activity_get","activity_status"]\n'
START = '2026-08-28T12:00:00Z'
END = '2026-08-28T13:00:00Z'


def cases():
    result = []
    def add(name, args):
        result.append((name, args))
    def search(**changes):
        values = dict(profile='activity-oracle', limit='1', **{'max-tokens': '100'}, from_=START, to=END)
        query = changes.pop('query', 'editor')
        values.update(changes)
        return ['activity', 'search', *[f'--{key.replace("from_", "from")}={value}' for key, value in values.items()], query]
    for verb in ('search', 'get', 'status'):
        base = ['activity', verb, '--profile=activity-oracle']
        for name, extra in [('unknown', ['--unknown']), ('help', ['--help']), ('missing-int', ['--limit']), ('missing-text', ['--from']), ('bad-syntax', ['---bad']), ('equals', ['--=x']), ('literal', ['--', 'query', '--max-tokens=10']), ('post-positional', ['id', '--max-tokens=100']), ('bare-hyphen', ['-'])]:
            add(f'{verb}-{name}', base + extra)
        for integer in ('', 'bad', '1.0', '-1', '0', '4001', '9223372036854775808', '-9223372036854775809', '0x_', '_1', '1_', '0xg', '09', '0b2', '1__2', '１２', '1\n', '999999999999999999999999999999999x'):
            add(f'{verb}-int-{len(result)}', base + [f'--max-tokens={integer}'] + ([] if verb=='status' else ['editor']))
        for spelling in ('+100', '0x64', '0b1100100', '0o144', '0144', '1_00', '0x_64'):
            add(f'{verb}-number-{spelling}', base + [f'--max-tokens={spelling}'] + (['--limit=1', f'--from={START}', f'--to={END}', 'editor'] if verb=='search' else ['missing-id'] if verb=='get' else []))
    for field in ('from_', 'to'):
        for value in ('', 'invalid', '2026', '2026-1-02T00:00:00Z', '2026-01-2T00:00:00Z', '2026-00-02T00:00:00Z', '2026-13-02T00:00:00Z', '2026-02-30T00:00:00Z', '2026-01-00T00:00:00Z', '2026-01-02 00:00:00Z', '2026-01-02T24:00:00Z', '2026-01-02T00:60:00Z', '2026-01-02T00:00:60Z', '2026-01-02T00:00:00', '2026-01-02T00:00:00z', '2026-01-02T00:00:00+25:00', '2026-01-02T00:00:00+00:61', '2026-01-02T00:00:00+25:61', '2026-01-02T00:00:00+25:ab', '2026-01-02T00:00:00+ab:61', '2026-01-02T00:00:00?25:61', '2026-01-02T00:00:00+0a:00', '2026-01-02T00:00:00+0000', '2026-01-02T00:00:00Ztail', '2026-01-02T00:00:00.Z', '🌻', '2026-01-02T00:00:00Z\n'):
            add(f'time-{field}-{len(result)}', search(**{field: value}))
    for value in ('2026-08-28T2:00:00Z', '2026-08-28T12:00:00,123456789123Z', '2026-08-28T12:00:00+24:00', '2026-08-28T12:00:00+00:60'):
        add(f'accepted-time-{len(result)}', search(from_=value))
    for change in ({'limit': '0'}, {'limit': '51'}, {'query': ''}, {'query': ' '}, {'query': 'x'*513}, {'from_': END, 'to': START}, {'from_': START, 'to': START}, {'from_': '0001-01-01T00:00:00Z', 'to': '9999-01-01T00:00:00Z'}):
        add(f'bounds-{len(result)}', search(**change))
    for name, args in [('duplicate-profile', ['activity','status','--profile=absent','--profile=activity-oracle','--max-tokens=100']), ('duplicate-profile-denied', ['activity','status','--profile=activity-oracle','--profile=absent','--max-tokens=100']), ('single-profile', ['activity','status','-profile=activity-oracle','--max-tokens=100']), ('unknown-verb', ['activity','help','--profile=activity-oracle']), ('get-ignored-search-flags', ['activity','get','--profile=activity-oracle','--max-tokens=100','--from=invalid','--limit=-1','missing-id'])]:
        add(name,args)
    # Keep source-bound error precedence when multiple parts are invalid:
    # unsigned overflow wins while scanning; underscore syntax is checked
    # before signed range, and raw invalid bytes are examined in input order.
    integers = ('+', '-', '0', '0x', '0b', '0o', '0_1', '0__1', '0b_1', '0b__1',
                '0o_1', '0o__1', '0x_1', '0x__1', '_1', '1__2', '1_',
                '9223372036854775807', '9223372036854775808',
                '-9223372036854775808', '-9223372036854775809',
                '9223372036854775808_', '18446744073709551615_',
                '18446744073709551616_', '1__9999999999999999999999999999',
                '0x__ffffffffffffffffffff', '0xffffffffffffffff_',
                '0x10000000000000000_', '0o__77777777777777777777777',
                '0b__' + '1'*65, '-18446744073709551616_',
                '+18446744073709551616_')
    for flag in ('limit', 'max-tokens'):
        for index, value in enumerate(integers):
            add(f'integer-precedence-{flag}-{index}', ['activity', 'status',
                '--profile=activity-oracle', '--max-tokens=100', f'--{flag}={value}'])
    if os.name!='nt':
        for raw in (b'\xff', b'\xe2\x82', b'\xef\xbf\xbd'):
            add(f'raw-time-{len(result)}', ['activity','search','--profile=activity-oracle','--limit=1','--max-tokens=100',b'--from='+raw,f'--to={END}','editor'])
            add(f'raw-int-{len(result)}', ['activity','status','--profile=activity-oracle',b'--max-tokens='+raw])
            add(f'raw-flag-{len(result)}', ['activity','status','--profile=activity-oracle',b'--'+raw])
            add(f'raw-query-{len(result)}', ['activity','search','--profile=activity-oracle','--limit=1','--max-tokens=100',f'--from={START}',f'--to={END}',raw*257])
        for flag in ('limit', 'max-tokens'):
            for index, value in enumerate((b'9999999999999999999999999999\xff',
                    b'\xff9999999999999999999999999999', b'9223372036854775808\xff',
                    b'-9223372036854775809\xff', b'0x10000000000000000\xe2\x82',
                    b'0x\xff10000000000000000', b'0x10000000000000000\xef\xbf\xbd',
                    b'18446744073709551615\xff')):
                add(f'raw-integer-precedence-{flag}-{index}', ['activity', 'status',
                    '--profile=activity-oracle', '--max-tokens=100',
                    b'--'+flag.encode()+b'='+value])
    assert len(result)>150 and len({name for name,_ in result})==len(result)
    return result


def invoke(binary, args, root):
    home = root/'home'
    config = home/'.config'
    profiles = config/'symbrain/profiles'
    profiles.mkdir(parents=True)
    (profiles/'activity-oracle.toml').write_text(PROFILE)
    env = {key: os.environ[key] for key in ('SystemRoot', 'windir', 'ComSpec', 'PATHEXT') if key in os.environ}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(config), XDG_DATA_HOME=str(root/'data'), XDG_CACHE_HOME=str(root/'cache'), XDG_STATE_HOME=str(root/'state'), SYMBRAIN_GO_BINARY=str(root/'absent-go'))
    output = subprocess.run([str(binary), *args], env=env, capture_output=True, timeout=20)
    return {'exit': output.returncode, 'stdout_base64': base64.b64encode(output.stdout).decode(), 'stderr_base64': base64.b64encode(output.stderr).decode()}


def freeze(data):
    output = dict(go_oracle_ref=data['go_oracle_ref'], go_binary_sha256=data['go_binary_sha256'], cases=[])
    for observation in data['observations']:
        args = [arg if isinstance(arg,str) else dict(hex=base64.b64decode(arg['raw_base64']).hex()) for arg in observation['args']]
        go = observation['go']
        output['cases'].append(dict(name=observation['name'], args=args, exit=go['exit'], stdout_hex=base64.b64decode(go['stdout_base64']).hex(), stderr_hex=base64.b64decode(go['stderr_base64']).hex()))
    return output


def main():
    go, rust, report = map(Path, sys.argv[1:])
    assert go.is_file() and rust.is_file(), 'actual Go and native executables required'
    observations = []
    for name, args in cases():
        with tempfile.TemporaryDirectory(prefix='activity-767-') as temp:
            root = Path(temp)
            left = invoke(go, args, root/'go')
            right = invoke(rust, args, root/'rust')
            observations.append(dict(name=name, args=[a if isinstance(a,str) else {"raw_base64":base64.b64encode(a).decode()} for a in args], go=left, rust=right, matches=left==right))
            if left!=right:
                print(f'FAIL {name}: real process output or exit mismatch', file=sys.stderr)
    source = ['rust/symbrain-cli/src/activity_cli.rs', 'rust/symbrain-cli/src/activity_args.rs', 'rust/symbrain-cli/src/activity_time.rs', 'rust/symbrain-cli/src/lib.rs', 'scripts/activity-validation-oracle/replay.py', 'scripts/activity-validation-oracle/controls.py', 'scripts/activity-validation-oracle/run.sh', 'rust/symbrain-activity/src/lib.rs', 'rust/symbrain-cli/tests/activity_validation_tests.rs']
    data = dict(go_oracle_ref='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c', candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(), candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),runtime=platform.platform(), total=len(observations), matches=all(o['matches'] for o in observations), observations=observations, go_binary_sha256=hashlib.sha256(go.read_bytes()).hexdigest(), rust_binary_sha256=hashlib.sha256(rust.read_bytes()).hexdigest(), candidate_source_sha256={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in source})
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(data,indent=2)+'\n')
    Path(str(report)+'.fixture.json').write_text(json.dumps(freeze(data),indent=2)+'\n')
    print(f"{sum(o['matches'] for o in observations)}/{len(observations)} real activity validation cases match")
    return 0 if data['matches'] else 1

if __name__=='__main__':
    raise SystemExit(main())
