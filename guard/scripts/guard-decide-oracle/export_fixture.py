#!/usr/bin/env python3
"""Export the 33 acceptance observations from a validated real Go capture."""
import argparse
import base64
import json
from pathlib import Path
import oracle


def main():
    p = argparse.ArgumentParser(); p.add_argument('--output', type=Path, required=True); p.add_argument('--check', action='store_true'); a = p.parse_args()
    m = oracle.validate_manifest(oracle.EVIDENCE / 'manifest.json')
    cases = json.loads(oracle.CASE_FILE.read_bytes())['cases']
    rows = []
    for c, result in zip(cases, m['results'], strict=True):
        if c.get('boundary_note'): continue
        stdout = oracle.evidence_path(oracle.EVIDENCE, result['stdout']['path'], 'stdout').read_bytes()
        if c['id'] == 'audit-failure':
            # The only path-bearing response: normalize the run-owned file
            # prefix, preserving the entire Go diagnostic and suffix verbatim.
            bad = str((oracle.EVIDENCE / result['stdout']['path']).parent / 'data-is-a-file')
            assert bad.encode() in stdout
            stdout = stdout.replace(bad.encode(), b'__AUDIT_FAILURE_PATH__')
        stderr = oracle.evidence_path(oracle.EVIDENCE, result['stderr']['path'], 'stderr').read_bytes()
        rows.append(dict(id=c['id'], exit_code=result['exit_code'], input_base64=base64.b64encode(oracle.raw_input(c,m['launch_timestamp'])).decode(), stdout_base64=base64.b64encode(stdout).decode(), stderr_base64=base64.b64encode(stderr).decode()))
    assert len(rows) == 33
    sources = ['cmd/symbrain/cmd_guard.go','cmd/symbrain/main.go','guard/cmd/symguard/decide/command.go']
    fixture=dict(schema_version=1, source_commit=oracle.PINNED_COMMIT_SHA, source_files={p:m['source_identity']['source_before'][p] for p in sources}, cases_sha256=m['cases']['sha256'], generator_sha256=m['harness']['sha256'], cases=rows)
    payload=(json.dumps(fixture,indent=2,sort_keys=True)+'\n').encode()
    if a.check: assert a.output.read_bytes() == payload, 'fixture drift'
    else: a.output.write_bytes(payload)
    print('PASS 33 real Go acceptance observations')

if __name__ == '__main__': main()
