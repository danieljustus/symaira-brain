#!/usr/bin/env python3
"""Healthy governance callbacks and failed Set output, with full committed state."""
import argparse
import base64
from contextlib import closing
import http.server
import json
import os
import signal
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import threading
import time
import replay
import writes


CALLBACKS = (
    ('kind-ignored', "CREATE TRIGGER fixture_kind BEFORE UPDATE OF kind ON memories BEGIN SELECT RAISE(IGNORE); END", '', False),
    ('stage-ignored', "CREATE TRIGGER fixture_stage BEFORE UPDATE OF review_status ON memories BEGIN SELECT RAISE(IGNORE); END", 'reference', False),
    ('removed-in-set-audit', "CREATE TRIGGER fixture_audit AFTER INSERT ON audit_log WHEN new.action='set' BEGIN DELETE FROM memories WHERE id=new.memory_id; END", '', True),
)


def committed_state(state, before, interval, expected, removed):
    events = [row for row in state['audit_log'] if row['action'] == 'set']
    assert len(events) == 1, 'the previously committed set audit must survive'
    identity = events[0]['memory_id']
    assert writes.UUID.fullmatch(identity), ('audited primary UUID', identity)
    if not removed:
        stable, bindings = writes.canonical(state, before, interval, expected, identity)
    else:
        assert state['memories'] == [], 'audit callback removed the primary row'
        # No missing row is fabricated for comparison. Bind the validated ID
        # through the actual set audit and every actual relationship column.
        stable, bindings = writes.canonical(state, before, interval, expected)
        assert identity not in bindings['ids'], 'primary and audit IDs must differ'
        bindings['ids'][identity] = '$memory'
        for rows in stable.values():
            for row in rows:
                for field in ('id', 'memory_id', 'entity_id', 'target_id'):
                    if row.get(field) == identity:
                        row[field] = '$memory'
    return identity, stable, bindings


def execute(go, rust, report):
    cases = [(name + '-' + fmt, trigger, kind, removed, fmt, False)
             for name, trigger, kind, removed in CALLBACKS for fmt in ('json', 'table')]
    unix_sink = os.name == 'posix' and Path('/dev/full').exists()
    if unix_sink:
        cases += [('metadata-output-full-' + fmt, None, 'reference', False, fmt, True)
                  for fmt in ('json', 'table')]
    unix_pipe = os.name == 'posix'
    if unix_pipe:
        cases += [('metadata-output-closed-pipe-' + fmt, None, 'reference', False, fmt, 'pipe')
                  for fmt in ('json', 'table')]
    records = []
    server = http.server.HTTPServer(('127.0.0.1', 0), writes.Embeddings)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for name, trigger, kind, removed, fmt, sink in cases:
            with tempfile.TemporaryDirectory(prefix='memory-write-failures-') as temporary:
                root = Path(temporary)
                env = writes.environment(root)
                env.update(SYMMEMORY_CONFLICT_ENABLED='false', SYMMEMORY_OLLAMA_MODEL='owned-model',
                           SYMMEMORY_OLLAMA_URL=f'http://127.0.0.1:{server.server_port}/api/embeddings')
                path = root / 'memory.db'
                assert replay.output(go, ['list', '--db', str(path)], root, env)['exit'] == 0
                if trigger:
                    with closing(sqlite3.connect(path)) as db, db:
                        db.execute(trigger)
                saved = root / 'before.sqlite'
                with closing(sqlite3.connect(path)) as db, closing(sqlite3.connect(saved)) as backup:
                    db.backup(backup)
                before = writes.snapshot(path)
                args = ['set', 'hello world', '--kind', 'fact', '--staged', '--db', str(path),
                        '--metadata', '{"fixture":"value"}', '--output', fmt]
                expected = dict(author='cli:symbrain', kind=kind, staged=bool(sink), metadata={'fixture': 'value'})
                pair = []
                for binary in (go, rust):
                    with closing(sqlite3.connect(saved)) as backup, closing(sqlite3.connect(path)) as db:
                        backup.backup(db)
                    writes.Embeddings.requests = []
                    writes.Embeddings.mode = 'success'
                    start = time.time_ns()
                    if sink == 'pipe':
                        read_fd, write_fd = os.pipe()
                        os.close(read_fd)
                        with os.fdopen(write_fd, 'wb', buffering=0) as output:
                            result = subprocess.run([str(binary), 'memory', *args], cwd=root, env=env,
                                                    stdout=output, stderr=subprocess.PIPE, timeout=12)
                        transcript = dict(exit=result.returncode, stdout=None,
                                          stderr=base64.b64encode(result.stderr).decode())
                    elif sink:
                        with open('/dev/full', 'wb', buffering=0) as output:
                            result = subprocess.run([str(binary), 'memory', *args], cwd=root, env=env,
                                                    stdout=output, stderr=subprocess.PIPE, timeout=12)
                        transcript = dict(exit=result.returncode, stdout=None,
                                          stderr=base64.b64encode(result.stderr).decode())
                    else:
                        transcript = replay.output(binary, args, root, env)
                    end = time.time_ns()
                    state = writes.snapshot(path)
                    try:
                        identity, stable, bindings = committed_state(state, before, (start, end), expected, removed)
                        stderr = base64.b64decode(transcript['stderr'])
                        if sink == 'pipe':
                            assert stderr == b'', 'actual stdout SIGPIPE must stay quiet'
                            comparable_stderr = stderr
                        elif sink:
                            assert stderr == b'symbrain memory set: format output: write /dev/stdout: no space left on device\n'
                            comparable_stderr = stderr
                        else:
                            assert base64.b64decode(transcript['stdout']) == b'', 'failed governance emits no success reply'
                            assert stderr == f'symbrain memory set: store memory: memory not found: {identity}\n'.encode()
                            comparable_stderr = stderr.replace(identity.encode(), b'$memory')
                        expected_exit = -signal.SIGPIPE if sink == 'pipe' else 1
                        assert transcript['exit'] == expected_exit, 'failure must propagate to CLI exit'
                        requests = list(writes.Embeddings.requests)
                        assert requests == [dict(path='/v1/embeddings', body=dict(model='owned-model', input=['hello world']))]
                        fts = writes.audit_fts(path)
                        assert fts == sorted((row['id'], row['content'], row['scope']) for row in state['memories'])
                    except Exception as error:
                        writes.save_json(report, dict(failed_case=name, error=str(error), transcript=transcript,
                                                       state=state, before=before, interval_ns=[start,end], other_processes=pair))
                        raise
                    pair.append(dict(transcript=transcript, state=state, bindings=bindings, interval_ns=[start,end], fts=fts,
                                     comparison=dict(exit=transcript['exit'], stdout=transcript['stdout'],
                                                     stderr=base64.b64encode(comparable_stderr).decode(), state=stable, requests=requests)))
                records.append(dict(name=name, trigger=trigger, args=args, before=before, go=pair[0], rust=pair[1],
                                    match=pair[0]['comparison'] == pair[1]['comparison']))
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    document = dict(go_binary_sha256=replay.digest(go.read_bytes()), rust_binary_sha256=replay.digest(rust.read_bytes()),
                    cases=records, passed=sum(row['match'] for row in records), total=len(records),
                    unix_sink_executed=unix_sink, unix_closed_pipe_executed=unix_pipe,
                    portable_failing_writer_test='memory_cli::write_output::tests::output_failure_reports_error_after_committed_set')
    writes.save_json(report, document)
    assert document['passed'] == document['total'], 'actual Go/native failure state differs'
    print(json.dumps(dict(passed=document['passed'], total=document['total'], unix_sink_executed=unix_sink)))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--go', type=Path, required=True)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    execute(args.go.resolve(), args.rust.resolve(), args.report)
