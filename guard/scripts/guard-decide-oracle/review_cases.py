#!/usr/bin/env python3
"""Regenerate review counterexamples with the verified historical Go binary."""
import argparse

import json
from pathlib import Path
import tempfile

import oracle

CASES = [
    ('scalar-null', '{"command":"open","command":null,"risk_class":"low"}'),
    ('folded-keys', '{"Command":"open","Risk_Class":"low"}'),
    ('warnings-null-element', '{"command":"open","risk_class":"low","warnings":[null]}'),
    ('warnings-number', '{"command":"open","risk_class":"low","warnings":[123]}'),
    ('zero-deadline', '{"command":"open","risk_class":"low","deadline":"0001-01-01T00:00:00Z"}'),
    ('empty-deadline', '{"command":"open","risk_class":"low","deadline":""}'),
    ('ipv6-zone', '{"command":"open","risk_class":"critical","domain":"::1%lo0"}'),
    ('quoted-bell', '{"command":"open","risk_class":"bad\\u0007class"}'),
    ('unicode-empty', '\u00a0\u3000'),
    ('top-level-array', '[]'),
    ('scalar-number', '{"command":123}'),
    ('deadline-null-retains', '{"command":"open","risk_class":"low","deadline":"2000-01-01T00:00:00Z","deadline":null}'),
    ('warnings-null-retains', '{"command":"open","risk_class":"high","warnings":["danger"],"warnings":[null]}'),
    ('zero-offset', '{"command":"open","risk_class":"low","deadline":"0001-01-01T01:00:00+01:00"}'),
    ('empty-zone', '{"command":"open","risk_class":"critical","domain":"::1%"}'),
    ('kelvin-key', '{"command":"open","ris\u212a_class":"low"}'),
    ('warnings-reused-tail', '{"command":"open","risk_class":"high","warnings":["one","two"],"warnings":["x"],"warnings":[null,null]}'),
    ('warnings-empty-resets', '{"command":"open","risk_class":"high","warnings":["danger"],"warnings":[],"warnings":[null]}'),
    ('trailing-x', '{"command":"open"}x'),
    ('trailing-whitespace-x', '{"command":"open"} \n\tx'),
    ('trailing-object', '{"command":"open"}{}'),
    ('trailing-array', '{"command":"open"}[]'),
    ('trailing-unicode', '{"command":"open"}é'),
    ('trailing-control', '{"command":"open"}\u0007'),
    ('trailing-quote', '{"command":"open"}"'),
    ('trailing-backslash', '{"command":"open"}\\\\'),
    ('trailing-whitespace-only', '{"command":"open"} \n\t'),
    ('trailing-first-line-followed-newline', '{"command":"open"}x\n{"command":"ignored"}'),
    ('trailing-first-line-unicode-followed-newline', '{"command":"open"}é\n{"command":"ignored"}'),
    ('trailing-later-line-followed-newline', '{"command":"ignored"}\n{"command":"open"}x\n{"command":"ignored"}'),
    ('trailing-later-line-unicode-followed-newline', '{"command":"ignored"}\n{"command":"open"}é\n{"command":"ignored"}'),
    ('trailing-crlf-later-line', '{"command":"ignored"}\r\n{"command":"open"}x\r\n{"command":"ignored"}'),
    ('trailing-crlf-whitespace-only', '{"command":"open"} \r\n\t'),
    ('trailing-later-line-whitespace-only', '{"command":"ignored"}\n{"command":"open"} \n\t'),
    ('trailing-later-line-control-followed-newline', '{"command":"ignored"}\n{"command":"open"}\u0007\n{"command":"ignored"}'),
]
PIN = oracle.PINNED_COMMIT_SHA
TRUSTED_BINARY = '0a10262e5cf4c08fa653df51736d192a3feb534432f36cd2deb83afbc7a2f547'
SOURCES = ['cmd/symbrain/cmd_guard.go', 'cmd/symbrain/main.go', 'guard/cmd/symguard/decide/command.go', 'guard/internal/model/event.go']


def generate(evidence):
    manifest = json.loads((evidence / 'manifest.json').read_bytes())
    assert manifest['source_identity']['commit_sha'] == PIN
    identity = oracle.provenance(evidence / 'pinned-tree', expected_commit_sha=PIN)
    hashes = identity['source_before']
    assert isinstance(hashes, dict)
    assert hashes == manifest['source_identity']['source_before']
    binary = oracle.evidence_path(evidence, manifest['binary']['path'], 'binary')
    assert oracle.digest(binary.read_bytes()) == TRUSTED_BINARY == manifest['binary']['sha256']
    assert binary.stat().st_size == manifest['binary']['bytes']
    cases = []
    for case_id, raw in CASES:
        with tempfile.TemporaryDirectory(prefix='guard-review-', dir='/private/tmp') as temp:
            runtime = Path(temp)
            env = {'PATH': '/usr/bin:/bin', 'HOME': str(runtime / 'home'), 'XDG_DATA_HOME': str(runtime / 'data'),
                   'XDG_CONFIG_HOME': str(runtime / 'config'), 'XDG_CACHE_HOME': str(runtime / 'cache'),
                   'TMPDIR': temp, 'TZ': 'UTC', 'LANG': 'C', 'LC_ALL': 'C'}
            result = oracle.run_bounded([str(binary), 'guard', 'decide'], cwd=oracle.ROOT, env=env, stdin=raw.encode(), timeout=5)
            assert result.returncode == 0 and not getattr(result, 'timed_out', True) and not result.stderr
            audit = json.loads((runtime / 'data/symguard/audit.log').read_bytes())
            del audit['id'], audit['decided_at']
            cases.append({'id': case_id, 'input': raw, 'stdout': result.stdout.decode(), 'audit_fields': audit})
    return {'schema_version': 1, 'source_commit': PIN,
            'source_files': {p: hashes[p] for p in SOURCES},
            'generator_sha256': oracle.digest(Path(__file__).read_bytes()), 'binary_sha256': TRUSTED_BINARY, 'cases': cases}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--evidence-root', type=Path, default=oracle.EVIDENCE)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    payload = (json.dumps(generate(args.evidence_root), indent=2, ensure_ascii=True) + '\n').encode()
    if args.check:
        if args.output.read_bytes() != payload:
            raise SystemExit('review fixture differs from pinned Go output')
    else:
        if args.output.exists():
            raise SystemExit('refusing to overwrite existing fixture; use a fresh output path')
        args.output.write_bytes(payload)
    print(f'PASS {len(CASES)} source-bound review cases')


if __name__ == '__main__':
    main()
