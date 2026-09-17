#!/usr/bin/env python3
"""F01-F05 cumulative production-binary regression, including audit locations.

Only id/decided_at are removed from audit comparison. All response bytes, exit
codes, stderr, audit keys/types/values and selected audit locations must match.
The optional fixture is captured from Go, never authored expected output.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile

import sys

SCRIPTS = Path(__file__).resolve().parents[3] / "scripts"
sys.path.insert(0, str(SCRIPTS))
from external_env import ensure_external_environment

ensure_external_environment(__file__)


HERE = Path(__file__).resolve().parent
SOURCE_FILES = (
    "cmd/symbrain/cmd_guard.go",
    "cmd/symbrain/main.go",
    "guard/cmd/symguard/decide/command.go",
)
FIXTURE_VALIDATION_BASIS = {
    "producer": "immutable Go binary",
    "argv": ["guard", "decide"],
    "compared": ["response", "audit"],
    "volatile_audit_fields": ["id", "decided_at"],
}


def digest(value):
    return hashlib.sha256(value).hexdigest()


def source_file_hashes(source_root):
    root = Path(source_root).resolve()
    if not root.is_dir():
        raise AssertionError(f"source root is not a directory: {root}")
    hashes = {}
    for relative in SOURCE_FILES:
        path = root / relative
        if not path.is_file():
            raise AssertionError(f"immutable source file is missing: {relative}")
        hashes[relative] = digest(path.read_bytes())
    return hashes


def fixture_payload(
    fixtures,
    *,
    source_root,
    validation_script,
    oracle_commit,
    go_toolchain,
    generator_sha256,
    validation_basis_sha256,
):
    validation_script = Path(validation_script).resolve()
    if not validation_script.is_file():
        raise AssertionError(f"validation basis is missing: {validation_script}")
    case_ids = [case["id"] for case in fixtures]
    if len(case_ids) != len(set(case_ids)):
        raise AssertionError("fixture case ids are not unique")
    document = {
        "schema_version": 2,
        "oracle_commit": oracle_commit,
        "source_files": source_file_hashes(source_root),
        "go_toolchain": go_toolchain,
        "generator_sha256": generator_sha256,
        "validation_basis": FIXTURE_VALIDATION_BASIS,
        "validation_basis_sha256": validation_basis_sha256,
        "case_count": len(fixtures),
        "case_ids": case_ids,
        "cases": fixtures,
    }
    return (json.dumps(document, indent=2) + "\n").encode()


def check_fixture(path, generated):
    path = Path(path).resolve()
    if not path.is_file():
        raise AssertionError(f"checked-in raw-byte fixture is missing: {path}")
    if path.read_bytes() != generated:
        raise AssertionError(f"checked-in raw-byte fixture drift: {path}")


def cases():
    out = []
    def raw(name, value): out.append((name, value))
    def request(name, **kw): raw(name, json.dumps(dict(command='open', risk_class='low') | kw, separators=(',', ':')).encode())
    request('normal')
    # These are deliberately raw bytes outside JSON strings. The frozen Go
    # 1.26.7 checkpoint quotes the byte as a Unicode escape in both diagnostic
    # locations; keep both minimal controls in the production corpus.
    raw('raw-byte-top-level-80', b'\x80')
    raw('raw-byte-trailing-80', b'{"command":"open","risk_class":"low"}\x80')
    for name, value in [('invalid-json', b'{]'), ('invalid-utf8', b'{"command":"\xff","risk_class":"low"}'), ('unknown-large-number', b'{"command":"open","risk_class":"low","extra":1e999}')]: raw(name, value)
    dates = ['2099-01-01t00:00:00z','2099-01-01 00:00:00Z','2099-01-01T00:00:60Z','2099-01-01T00:00:00Z', '2099-01-01T00:00:00z', '2099-01-01T24:00:00Z','2099-01-01T00:60:00Z','2099-13-01T00:00:00Z','2099-02-30T00:00:00Z','2099-01-00T00:00:00Z','2099-01-01T0:00:00Z','2099-01-01T00:00:00,123Z','2099-01-01T00:00:00.123456789012Z','2099-01-01T00:00:00+24:00','2099-01-01T00:00:00+00:60','2099-01-01T00:00:00+25:00','2099-01-01T00:00:00+00:61','2099-01-01T00:00:00Zextra','0001-01-01T00:00:00Z','0001-01-01T01:00:00+01:00','0001-01-01T00:00:00.000000001Z','2000-01-01T00:00:00Z', '', 'bad']
    for i, value in enumerate(dates): request(f'deadline-{i}', deadline=value)
    for risk in ['high','critical']:
        for value in dates[:3]: request('malformed-'+risk+'-'+str(dates.index(value)), deadline=value, risk_class=risk, domain='::ffff:127.0.0.1')
    for i, value in enumerate([None, [], {}, 1e10, True]): request(f'deadline-type-{i}', deadline=value)
    for i, body in enumerate([
        '"deadline":null,"deadline":"2099-01-01T00:00:00Z"',
        '"deadline":"2099-01-01T00:00:00Z","deadline":null',
        '"deadline":"bad","deadline":"2099-01-01T00:00:00Z"',
        '"deadline":"2099-01-01T00:00:00Z","deadline":"bad"',
        '"command":4,"deadline":"bad"', '"deadline":"bad","command":4',
        '"deadline":"2099-01-01T00:00:00\\u005a"',
        '"deadline":"bad","extra":1e999',
        '"extra":{"large":[1e999]},"warnings":[]',
        '"warnings":[1e999]', '"command":1e999',
        '"warnings":["x",null],"warnings":[null]',
        '"warnings":["x"],"warnings":[],"warnings":[null]',
    ]): raw(f'duplicate-type-{i}', ('{"command":"open","risk_class":"low",'+body+'}').encode())
    for i, domain in enumerate(['::ffff:127.0.0.1','::ffff:127.255.255.254','::ffff:7f00:2','::ffff:127.0.0.1%lo0','::ffff:128.0.0.1','::ffff:0.0.0.0','::127.0.0.1','127.0.0.1','127.1.2.3','::1','::1%lo0','::1%','::1%a%b','127.0.0.1%lo0','[::1]','localhost','*.localhost','0.0.0.0','::','192.168.1.1','::ffff:127.00.0.1']):
        request(f'loopback-{i}', domain=domain, risk_class='critical')
        request(f'loopback-warning-{i}', domain=domain, risk_class='critical', warnings=['danger'])
    for i, warnings in enumerate([None, [], [''], [' '], ['x'], [None], [4], {}, 'x']): request(f'warnings-{i}', warnings=warnings)
    for i, risk in enumerate(['low','medium','high','critical','unknown','',None]): request(f'risk-{i}', risk_class=risk)
    invalid = [b'', b' ', b'{', b'{} x', b'{"a" 1}', b'{"a":1 x}', b'{"a":1,}', b'[1,]', b'[1 x]', b'{"a":truX}', b'{"a":nulL}', b'{"a":falsX}', b'{"a":01}', b'{"a":1.}', b'{"a":1e}', b'{"a":-}', b'{"a":"\\q"}', b'{"a":"\\u12xx"}', b'{"a":"\x01"}', b'\xff', b'1e999', b'[]', b'true', b'null']
    for i, value in enumerate(invalid): raw(f'json-{i}', value)
    for i, text in enumerate([b'\xed\xa0\x80', b'\xf0\x80', b'\\ud800', b'\\udc00', b'\\ud800\\udc00', b'\\\\ud800']): raw(f'unicode-{i}', b'{"command":"'+text+b'","risk_class":"low"}')
    for depth in [127,128,1000,9999,10000]: raw(f'depth-{depth}', b'{"command":"open","risk_class":"low","extra":'+b'['*depth+b'0'+b']'*depth+b'}')
    request('deadline-zone-double-overflow', deadline='2099-01-01T00:00:00+99:99')
    for i, text in enumerate([b'\xff', b'\\ud800', b'2099-01-01T00:00:00Z\\ud800', b'2099-01-01T00:00:00Z\xff']):
        raw(f'deadline-unicode-{i}', b'{"command":"open","risk_class":"low","deadline":"'+text+b'"}')
    assert len(out) == len({name for name, _ in out})
    return out


def run(binary, payload, root, mode='xdg'):
    root.mkdir(parents=True)
    for name in ['home','profile','data','config','cache','tmp']: (root/name).mkdir()
    env = {k:v for k,v in os.environ.items() if k.upper() in {'SYSTEMROOT','WINDIR'}}
    env.update(PATH='', HOME=str(root/'home'), USERPROFILE=str(root/'profile'), XDG_DATA_HOME=str(root/'data'), XDG_CONFIG_HOME=str(root/'config'), XDG_CACHE_HOME=str(root/'cache'), TMPDIR=str(root/'tmp'), TMP=str(root/'tmp'), TEMP=str(root/'tmp'), TZ='UTC', LANG='C', LC_ALL='C', SYMBRAIN_GO_BINARY=str(root/'missing-fallback'))
    expected = root/'data/symguard/audit.log'
    if mode != 'xdg':
        env.pop('XDG_DATA_HOME')
        if mode == 'no-home': env.pop('HOME')
        if mode == 'no-profile': env.pop('USERPROFILE')
        if mode == 'no-both': env.pop('HOME'); env.pop('USERPROFILE')
        home = env.get('USERPROFILE' if os.name == 'nt' else 'HOME')
        expected = Path(home)/'.local/share/symguard/audit.log' if home else root/'tmp/symguard/audit.log'
    result = subprocess.run([str(binary), 'guard', 'decide'], input=payload, cwd=root, env=env, capture_output=True, timeout=10)
    logs = sorted(str(p.relative_to(root)) for p in root.rglob('audit.log'))
    assert logs == [str(expected.relative_to(root))], (mode, logs, expected)
    data = expected.read_bytes().splitlines()
    assert len(data) == 1
    audit = json.loads(data[0]); assert audit['id'].startswith('evt_1_decide_'); assert audit['decided_at'].endswith('Z')
    del audit['id']; del audit['decided_at']
    return dict(exit=result.returncode, stdout_hex=result.stdout.hex(), stderr_hex=result.stderr.hex(), audit=audit, locations=logs)


def same(left, right):
    # JSON type identity matters: Python otherwise equates true with 1.
    return json.dumps(left, sort_keys=True) == json.dumps(right, sort_keys=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--go', type=Path, required=True)
    p.add_argument('--rust', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--fixture', type=Path)
    p.add_argument('--check-fixture', action='store_true')
    p.add_argument('--source-root', type=Path)
    p.add_argument('--validation-script', type=Path)
    p.add_argument('--oracle-commit', '--source-commit', dest='oracle_commit',
                   required=True)
    p.add_argument('--go-toolchain', required=True)
    p.add_argument('--generator-sha256', required=True)
    p.add_argument('--validation-basis-sha256', required=True)
    a = p.parse_args()
    if a.check_fixture and not a.fixture:
        p.error('--check-fixture requires --fixture')
    if a.fixture and (not a.source_root or not a.validation_script):
        p.error('--fixture requires --source-root and --validation-script')
    a.go = a.go.resolve(); a.rust = a.rust.resolve()
    if not a.go.is_file() or not a.rust.is_file():
        raise AssertionError('both Go and Rust binaries are required')
    hashes = {k: hashlib.sha256(v.read_bytes()).hexdigest() for k, v in [('go', a.go), ('rust', a.rust)]}
    assert hashes['go'] != hashes['rust'], 'same binary is not differential evidence'
    rows = []; fixtures = []
    with tempfile.TemporaryDirectory(prefix='guard-repair-') as temp:
        for index, (name, payload) in enumerate(cases()):
            row = {'id': name, 'input_hex': payload.hex()}
            for kind, binary in [('go', a.go), ('rust', a.rust)]: row[kind] = run(binary, payload, Path(temp)/str(index)/kind)
            row['pass'] = same(row['go'], row['rust']); rows.append(row)
            fixtures.append({'id': name, 'input_hex': payload.hex(), 'response': json.loads(bytes.fromhex(row['go']['stdout_hex'])), 'audit': row['go']['audit']})
        for mode in ['xdg', 'both', 'no-home', 'no-profile', 'no-both']:
            row: dict = {'id': 'resolver-'+mode}
            for kind, binary in [('go', a.go), ('rust', a.rust)]: row[kind] = run(binary, b'{"command":"open","risk_class":"low"}', Path(temp)/mode/kind, mode)
            row['pass'] = same(row['go'], row['rust']); rows.append(row)
    original = rows[0]['go']
    for key, value in [('exit', 99), ('stdout_hex', '00'), ('stderr_hex', '00'), ('locations', [])]:
        altered = copy.deepcopy(original); altered[key] = value; assert not same(original, altered)
    for key, value in [('decision', 'deny'), ('command', 1), ('risk_class', None), ('warnings', [])]:
        altered = copy.deepcopy(original); altered['audit'][key] = value; assert not same(original, altered)
    report = {'platform': platform.platform(), 'binary_sha256': hashes, 'oracle_commit': a.oracle_commit, 'go_toolchain': a.go_toolchain, 'cases': rows, 'count': len(rows), 'passed': sum(row['pass'] for row in rows)}
    a.report.parent.mkdir(parents=True, exist_ok=True); a.report.write_text(json.dumps(report, indent=2)+'\n')
    if a.fixture:
        payload = fixture_payload(
            fixtures,
            source_root=a.source_root,
            validation_script=a.validation_script,
            oracle_commit=a.oracle_commit,
            go_toolchain=a.go_toolchain,
            generator_sha256=a.generator_sha256,
            validation_basis_sha256=a.validation_basis_sha256,
        )
        if a.check_fixture:
            check_fixture(a.fixture, payload)
        else:
            a.fixture.parent.mkdir(parents=True, exist_ok=True)
            a.fixture.write_bytes(payload)
    print(json.dumps({k: v for k, v in report.items() if k != 'cases'}))
    for row in rows:
        if not row['pass']: print(json.dumps(row))
    return 0 if report['count'] == report['passed'] else 1


if __name__ == '__main__': raise SystemExit(main())
