#!/usr/bin/env python3
"""Reject actual test-ELF deadline/body/task lifetime defects at their exact boundary."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    binaries = {}
    for line in args.artifacts.read_text().splitlines():
        item = json.loads(line)
        if item.get('reason') != 'compiler-artifact' or not item.get('executable'):
            continue
        if not item['profile']['test'] or Path(item['manifest_path']).parent.name != 'symbrowse-fetch':
            continue
        if item['target']['kind'] == ['lib']:
            binaries['lib'] = Path(item['executable'])
        elif item['target']['name'] == 'proxy_uri' and item['target']['kind'] == ['test']:
            binaries['proxy'] = Path(item['executable'])
    assert set(binaries) == {'lib', 'proxy'}, 'exact owned test artifacts missing'
    cases = [
        ('retained-body', 'lib', 'honest::proxy_uri::tests::raw_response_body_deadline_drops_actual_owner_before_client_teardown', b'owned proxy remains connected'),
        ('leaked-task', 'lib', 'honest::proxy_uri::tests::raw_connection_task_guard_aborts_owned_socket_before_client_teardown', b'owned proxy remains connected'),
        ('timeout', 'proxy', 'raw_proxy_deadline_closes_connection_while_client_remains_alive', b'fetch did not finish at the controlled deadline'),
    ]
    records = []
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for mode, owner, name, expected in cases:
        binary = binaries[owner].resolve()
        for mutant in (False, True):
            env = dict(os.environ)
            env.pop('FETCH_PROXY_FIXTURE_CONTROL', None)
            if mutant:
                env['FETCH_PROXY_FIXTURE_CONTROL'] = mode
            command = [str(binary), name, '--exact', '--nocapture']
            result = subprocess.run(command, cwd=repo, env=env, capture_output=True, timeout=15)
            transcript = result.stdout + result.stderr
            accepted = result.returncode == 0 and b'1 passed; 0 failed' in transcript
            rejected = result.returncode == 101 and b'0 passed; 1 failed' in transcript and expected in transcript
            record = dict(mode=mode, mutant=mutant, binary=str(binary), binary_sha256=digest(binary), command=command,
                          exit=result.returncode, accepted=accepted, exact_rejection=rejected,
                          stdout_base64=base64.b64encode(result.stdout).decode(), stderr_base64=base64.b64encode(result.stderr).decode())
            records.append(record)
            # Preserve all actual failures before judging the intended assertion.
            document = dict(candidate_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
                            candidate_dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=repo)),
                            cargo_artifacts_sha256=digest(args.artifacts), observations=records)
            args.output.write_bytes((json.dumps(document, indent=2)+'\n').encode())
            assert rejected if mutant else accepted, (mode, mutant, record)
    print('Three real positive tests pass; three actual owner/clock mutants reject at their intended assertions')


if __name__ == '__main__':
    main()
