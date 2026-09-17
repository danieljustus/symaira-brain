#!/usr/bin/env python3
"""Compare actual guard decide binaries, including help/args and audit effects."""
import argparse
import datetime
import json
from pathlib import Path
import platform
import tempfile

import oracle
import review_cases


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--evidence-root', type=Path, required=True)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    assert not args.output.exists(), 'never overwrite evidence'
    evidence = args.evidence_root.resolve()
    manifest = json.loads((evidence / 'manifest.json').read_bytes())
    source = oracle.provenance(evidence / 'pinned-tree', expected_commit_sha=review_cases.PIN)
    assert source['source_before'] == manifest['source_identity']['source_before']
    go = oracle.evidence_path(evidence, manifest['binary']['path'], 'binary')
    assert oracle.digest(go.read_bytes()) == review_cases.TRUSTED_BINARY == manifest['binary']['sha256']
    binaries = {'go': go, 'rust': args.rust.resolve()}
    assert binaries['go'] != binaries['rust']
    assert oracle.digest(go.read_bytes()) != oracle.digest(binaries['rust'].read_bytes())
    cases = [(name, [], raw.encode()) for name, raw in review_cases.CASES]
    for name, flags in [('help', ['--help']), ('short-help', ['-h']), ('extra', ['extra']),
                        ('help-precedence', ['extra', '--help']), ('single-dash-not-help', ['-help'])]:
        cases.append((name, flags, b'{"command":"open","risk_class":"low"}'))
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    rows = []
    for name, flags, data in cases:
        row = {'id': name, 'flags': flags, 'input_sha256': oracle.digest(data)}
        for kind, binary in binaries.items():
            with tempfile.TemporaryDirectory(prefix='gcli-') as tmp:
                env = {'PATH': '/usr/bin:/bin', 'HOME': tmp + '/home', 'XDG_DATA_HOME': tmp + '/data',
                       'XDG_CONFIG_HOME': tmp + '/config', 'XDG_CACHE_HOME': tmp + '/cache',
                       'TMPDIR': tmp, 'TZ': 'UTC', 'LANG': 'C', 'LC_ALL': 'C'}
                command = [str(binary), 'guard', 'decide', *flags]
                result = oracle.run_bounded(command, cwd=oracle.ROOT, env=env, stdin=data, timeout=5)
                path = Path(tmp) / 'data/symguard/audit.log'
                audit = None
                if path.exists():
                    lines = path.read_bytes().splitlines()
                    assert len(lines) == 1, 'exactly one audit record'
                    audit = json.loads(lines[0])
                    assert audit['id'].startswith('evt_1_decide_')
                    assert audit['decided_at'].endswith('Z')
                    del audit['id'], audit['decided_at']
                row[kind] = {'exit': result.returncode, 'stdout': result.stdout.decode(),
                             'stderr': result.stderr.decode(), 'timed_out': result.timed_out, 'audit': audit}
        row['passed'] = row['go'] == row['rust'] and not row['go']['timed_out']
        rows.append(row)
    # Negative control goes through the exact same equality comparison.
    control = dict(rows[0]['rust'], exit=99)
    assert rows[0]['go'] != control
    report = {'source_commit': review_cases.PIN, 'cwd': str(oracle.ROOT), 'start': started,
              'end': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'platform': platform.platform(),
              'binaries': {name: {'path': str(path), 'sha256': oracle.digest(path.read_bytes())} for name, path in binaries.items()},
              'allowed_normalizations': ['audit id (format checked)', 'audit decided_at (UTC checked)'],
              'cases': rows, 'count': len(rows), 'passed': sum(row['passed'] for row in rows)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(f"PASS {report['passed']}/{report['count']}" if all(r['passed'] for r in rows) else json.dumps([r for r in rows if not r['passed']], indent=2))
    raise SystemExit(0 if all(row['passed'] for row in rows) else 1)


if __name__ == '__main__':
    main()
