#!/usr/bin/env python3
"""Additive real-process deterministic-conflict producer; SOURCE_ONLY until run.

Uses the existing pinned runner, isolated environments, actual Go-created DBs,
raw transcript capture and complete SQLite/FTS inspection. Never imports Rust
internals or supplies fabricated Go outcomes. Seed mutations are labelled inputs.
"""
import base64
import datetime
from contextlib import closing
import http.server
import json
import os
import re
import signal
from pathlib import Path
import sqlite3
import struct
import subprocess
import tempfile
import threading
import time
import replay
import writes


class Embeddings(writes.Embeddings):
    requests = []
    mutate: str | None = None
    database: Path | None = None

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        type(self).requests.append(dict(path=self.path, body=body))
        mutation, database = type(self).mutate, type(self).database
        if mutation:
            assert database is not None
            with closing(sqlite3.connect(database)) as db, db:
                db.execute(mutation)
        prompt = body['input'][0]
        head = {'hello world': [1.0, 0.0], 'hello sky': [1.0, 0.1],
                'hello band': [1.0, 0.5], 'hello closest': [1.0, 0.11],
                'hello other': [0.0, 1.0]}[prompt]
        payload = json.dumps({'data': [{'index': 0, 'embedding': head + [0.0] * 766}]}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def scenarios():
    # Analytic inputs straddle one final-score f32 midpoint; both ordinary
    # importance values round to the same f32 if hydration narrows too early.
    lower_score = struct.unpack('f', struct.pack('f', 0.7))[0]
    bits = struct.unpack('I', struct.pack('f', lower_score))[0]
    upper_score = struct.unpack('f', struct.pack('I', bits + 1))[0]
    center = ((lower_score + upper_score) / 2 - 0.6) * 50
    cases = [
        dict(name='default-empty', seeds=[], query='hello sky', outcome='new'),
        dict(name='exact-repeat', query='hello world', outcome='repeat', entities='Fresh'),
        dict(name='near-duplicate', query='hello sky', outcome='near'),
        dict(name='ambiguous-band', query='hello band', outcome='band'),
        dict(name='below-band', query='hello other', outcome='new'),
        dict(name='best-near-short-circuits-band', seeds=['hello world', 'hello closest', 'hello band'], query='hello sky', outcome='near', winner_target=1),
        dict(name='other-scope', query='hello world', scope='user', outcome='new'),
        dict(name='staged-bypass', query='hello world', staged=True, outcome='new'),
        dict(name='disabled-bypass', query='hello world', env={'SYMMEMORY_CONFLICT_ENABLED': 'false'}, outcome='new'),
        dict(name='configured-band', query='hello sky', env={'SYMMEMORY_CONFLICT_NEAR_DUP_THRESHOLD': '0.9999'}, outcome='band'),
        dict(name='configured-low-threshold', query='hello band', config='[conflict]\nnear_dup_threshold=0.85\n', outcome='near'),
        dict(name='normalized-threshold', query='hello sky', env={'SYMMEMORY_CONFLICT_NEAR_DUP_THRESHOLD': '0', 'SYMMEMORY_CONFLICT_MAX_CANDIDATES': '-1'}, outcome='near'),
        dict(name='configured-max-one', seeds=['hello world', 'hello band'], query='hello sky', importance=1,
             env={'SYMMEMORY_CONFLICT_MAX_CANDIDATES': '1', 'SYMMEMORY_CONFLICT_NEAR_DUP_THRESHOLD': '0.97'}, outcome='band', recalled=1),
        dict(name='duplicate-expired-other-space', query='hello world', mutation="tier='working',expires_at='2000-01-01 00:00:00 +0000 UTC',embedding_source='other'", outcome='repeat'),
        dict(name='checker-error-after-admission', query='hello world', on_request="UPDATE memories SET metadata='bad json'", outcome='new'),
        dict(name='feedback-error', query='hello sky', trigger="CREATE TRIGGER fixture BEFORE UPDATE OF access_count ON memories BEGIN SELECT RAISE(ABORT,'feedback denied'); END", outcome='near', feedback=False),
        dict(name='supersede-error', query='hello sky', trigger="CREATE TRIGGER fixture BEFORE UPDATE OF superseded_by ON memories BEGIN SELECT RAISE(ABORT,'supersession denied'); END", outcome='new'),
        dict(name='supersede-ignored', query='hello sky', trigger="CREATE TRIGGER fixture BEFORE UPDATE OF superseded_by ON memories BEGIN SELECT RAISE(IGNORE); END", outcome='near', superseded=False),
        dict(name='pending-audit-error', query='hello band', trigger="CREATE TRIGGER fixture BEFORE INSERT ON audit_log WHEN new.action='conflict_pending' BEGIN SELECT RAISE(ABORT,'audit denied'); END", outcome='new'),
        dict(name='repeat-kind-ignored', query='hello world', trigger="CREATE TRIGGER fixture BEFORE UPDATE OF kind ON memories BEGIN SELECT RAISE(IGNORE); END", outcome='repeat', failure=True),
        dict(name='removed-in-set-audit', query='hello sky', trigger="CREATE TRIGGER fixture AFTER INSERT ON audit_log WHEN new.action='set' BEGIN DELETE FROM memories WHERE id=new.memory_id; END", outcome='near', failure=True, removed=True),
        dict(name='checker-recall-error-after-admission', query='hello sky', on_request="UPDATE memories SET metadata='bad json'", outcome='new'),
        dict(name='checker-recall-time-after-admission', query='hello sky', on_request="UPDATE memories SET created_at='invalid time'",
             mutated_fields={'created_at': 'invalid time'}, scan_index=4, outcome='new'),
        dict(name='checker-recall-compound-after-admission', query='hello sky', on_request="UPDATE memories SET metadata='bad json',created_at='invalid time'",
             mutated_fields={'metadata': 'bad json', 'created_at': 'invalid time'}, scan_index=4, outcome='new'),
        dict(name='checker-duplicate-compound-after-admission', query='hello world', on_request="UPDATE memories SET metadata='bad json',created_at='invalid time'",
             mutated_fields={'metadata': 'bad json', 'created_at': 'invalid time'}, scan_index=9, outcome='new'),
        dict(name='supersede-audit-error', query='hello sky', trigger="CREATE TRIGGER fixture BEFORE INSERT ON audit_log WHEN new.action='supersede' BEGIN SELECT RAISE(ABORT,'audit denied'); END", outcome='new', superseded_row=True),
        dict(name='f64-close-ranking-cap', seeds=['hello world', 'hello closest'], query='hello sky', outcome='near',
             importance_values=[center - 1e-8, center + 1e-8], env={'SYMMEMORY_CONFLICT_MAX_CANDIDATES': '1'}, winner_target=1, recalled=1, recalled_target=1),
        dict(name='f64-large-ranking-cap', seeds=['hello world', 'hello closest'], query='hello sky', outcome='near',
             importance_values=[4e38, 8e38], env={'SYMMEMORY_CONFLICT_MAX_CANDIDATES': '1'}, winner_target=1, recalled=1, recalled_target=1),
    ]
    for label, mutation in [
        ('archived', "consolidation_status='archived'"), ('staged', "review_status='staged'"),
        ('retired', "retired_at='2000-01-01 00:00:00 +0000 UTC'"),
        ('superseded', "superseded_by='older-winner'"), ('other-space', "embedding_source='other'"),
        ('dimension', 'embedding_dim=3'), ('quantization', "embedding_quantization='binary'"),
    ]:
        cases.append(dict(name='exclude-' + label, query='hello sky', mutation=mutation, outcome='new'))
    if os.name == 'posix':
        cases.extend([
            dict(name='repeat-stderr-closed-pipe', query='hello world', outcome='repeat', entities='Fresh', stderr_pipe=True),
            dict(name='degradation-stderr-closed-pipe', query='hello world', outcome='new', stderr_pipe=True,
                 on_request="UPDATE memories SET metadata='bad json'", mutated_fields={'metadata': 'bad json'}),
        ])
    return [dict(case, format=fmt) for case in cases for fmt in ('json', 'table')]


def prepare(go, root, env, path, case):
    transcripts = []
    def run(args):
        result = replay.output(go, args + ['--db', str(path), '--json'], root, env)
        transcripts.append(dict(args=args, result=result))
        assert result['exit'] == 0, ('Go preparation failed', result)
        return json.loads(base64.b64decode(result['stdout']))
    run(['list'])
    # Probe the actual Go LSH for this query, then remove the probe via Go.
    probe = run(['set', case['query'], '--kind', 'reference'])['id']
    with closing(sqlite3.connect(path)) as db:
        lsh, query_embedding = db.execute('SELECT lsh_hash,embedding FROM memories WHERE id=?', (probe,)).fetchone()
    run(['delete', probe])
    ids = [run(['set', text, '--kind', 'reference', '--author', 'original'])['id']
           for text in case.get('seeds', ['hello world'])]
    with closing(sqlite3.connect(path)) as db, db:
        for index, identity in enumerate(ids):
            # Force LSH admission to isolate the cosine/ranking contract. These
            # are explicit scenario inputs, not observed natural LSH recall.
            db.execute("UPDATE memories SET lsh_hash=?,created_at='2000-01-01 00:00:00 +0000 UTC',updated_at='2000-01-01 00:00:00 +0000 UTC',importance=? WHERE id=?",
                       (lsh, 10 if case.get('importance') == index else 0, identity))
            if 'importance_values' in case:
                # Equal, real Go-produced vectors isolate ranking precision.
                # Put the lower weight first in recall order to expose ties.
                db.execute('UPDATE memories SET importance=?,embedding=?,created_at=? WHERE id=?',
                           (case['importance_values'][index], query_embedding,
                            ('2001' if index == 0 else '2000') + '-01-01 00:00:00 +0000 UTC', identity))
        if case.get('mutation'):
            db.execute('UPDATE memories SET ' + case['mutation'])
        if case.get('trigger'):
            db.execute(case['trigger'])
    return ids, transcripts


def canonical(state, before, interval, primary, mutated_fields):
    old = {table: {row['id']: row for row in rows if 'id' in row} for table, rows in before.items()}
    ids = {}
    if primary is not None and primary not in old['memories']:
        assert writes.UUID.fullmatch(primary), ('new primary UUID', primary)
        ids[primary] = '$memory'
    for table in ('entities', 'audit_log'):
        for index, row in enumerate(state[table]):
            if row['id'] not in old[table]:
                assert writes.UUID.fullmatch(row['id']) and row['id'] not in ids
                ids[row['id']] = '$' + table + ':' + str(index)
    start, end = interval
    transformed = json.loads(json.dumps(state))
    times = []
    temporal = {'memories': ('created_at', 'updated_at', 'valid_from', 'valid_to', 'last_access', 'prev_access'),
                'entities': ('created_at', 'updated_at'), 'audit_log': ('created_at',), 'sync_oplog': ('ts',)}
    old_events = {row['event_id'] for row in before['sync_oplog']}
    for table, fields in temporal.items():
        for row in transformed[table]:
            previous = old[table].get(row.get('id'), {})
            if table == 'sync_oplog' and row['event_id'] in old_events:
                continue
            for field in fields:
                value = row.get(field)
                if table == 'memories' and previous and field in mutated_fields:
                    # A declared post-admission input is literal, not a clock
                    # observation. Validate it and retain it in the comparison.
                    assert value == mutated_fields[field], ('mutation lost', field, row)
                    continue
                if value is None or value == previous.get(field):
                    continue
                at = writes.instant(value)
                # prev_access is a copied earlier observation, not a new time.
                if field == 'prev_access' and value == previous.get('last_access'):
                    continue
                tolerance = 10**9 if table == 'sync_oplog' else 0
                assert start - tolerance <= at <= end + tolerance, (table, field, value)
                times.append(dict(table=table, id=row.get('id', row.get('event_id')), field=field, literal=value, ns=at))
                row[field] = '$time:' + table + ':' + field
            if table == 'memories' and row['id'] not in old['memories']:
                raw = json.loads(row['metadata'])
                observed = writes.instant(raw['observed_at'])
                assert start // 10**9 * 10**9 <= observed <= end
                row['metadata'] = row['metadata'].replace('"observed_at":"' + raw['observed_at'] + '"', '"observed_at":"$observed"')
    for table, rows in transformed.items():
        for row in rows:
            for field in ('id', 'memory_id', 'entity_id', 'target_id', 'superseded_by'):
                if isinstance(row.get(field), str) and row[field] in ids:
                    row[field] = ids[row[field]]
            if table == 'audit_log' and row.get('detail') and row['action'] in ('supersede', 'conflict_pending'):
                detail = json.loads(row['detail'])
                for field in ('winner_id', 'candidate_id'):
                    if detail.get(field) in ids:
                        literal = json.dumps(detail[field])
                        row['detail'] = row['detail'].replace('"' + field + '":' + literal, '"' + field + '":' + json.dumps(ids[detail[field]]))
    shadows = [table for table in transformed if table.startswith('memories_fts_')]
    physical = {table: replay.digest(json.dumps(state[table], sort_keys=True).encode()) for table in shadows}
    for table in shadows:
        del transformed[table]
    for rows in transformed.values():
        rows.sort(key=lambda row: json.dumps(row, sort_keys=True))
    return transformed, dict(ids=ids, times=times, physical_fts_shadow_sha256=physical)


def observe(case, state, before, transcript, seed_ids):
    old_audits = {row['id'] for row in before['audit_log']}
    audits = [row for row in state['audit_log'] if row['id'] not in old_audits]
    if case.get('stderr_pipe'):
        assert transcript['exit'] == -signal.SIGPIPE, ('stderr must terminate at diagnostic', transcript)
        assert base64.b64decode(transcript['stdout']) == b'' and transcript['stderr'] is None
        assert not audits, ('diagnostic must precede persistence/callbacks', audits)
        assert {row['id'] for row in state['memories']} == set(seed_ids)
        assert state['entities'] == before['entities'] and state['memory_entities'] == before['memory_entities']
        for row in state['memories']:
            original = next(item for item in before['memories'] if item['id'] == row['id'])
            assert row['kind'] == original['kind'], 'stderr termination precedes governance'
        return None
    new_sets = [row for row in audits if row['action'] == 'set']
    repeat = case['outcome'] == 'repeat'
    assert len(new_sets) == (0 if repeat else 1), ('set audit count', audits)
    primary = seed_ids[0] if repeat else new_sets[0]['memory_id']
    stdout = base64.b64decode(transcript['stdout'])
    assert transcript['exit'] == int(case.get('failure', False)), ('conflict exit', transcript)
    if case.get('failure'):
        assert stdout == b''
    else:
        if case['format'] == 'json':
            assert json.loads(stdout) == dict(id=primary, scope=case.get('scope', 'global'), kind=case.get('kind', 'user'), staged=case.get('staged', False))
        else:
            status = 'staged for review' if case.get('staged') else 'stored'
            assert stdout == f"Memory {primary} ({case.get('scope', 'global')}, {case.get('kind', 'user')}, {status}).\n".encode()
    supersedes = [row for row in audits if row['action'] == 'supersede']
    pending = [row for row in audits if row['action'] == 'conflict_pending']
    assert len(supersedes) == int(case['outcome'] == 'near')
    assert len(pending) == int(case['outcome'] == 'band')
    rows = {row['id']: row for row in state['memories']}
    assert len(rows) == len(before['memories']) + (0 if repeat or case.get('removed') else 1)
    if not case.get('removed'):
        row = rows[primary]
        if not case.get('failure'):
            assert row['kind'] == case.get('kind', 'user')
        if not repeat:
            assert writes.instant(row['valid_from']) <= writes.instant(row['created_at']) <= writes.instant(row['updated_at'])
        else:
            original = next(item for item in before['memories'] if item['id'] == primary)
            for field in ('created_at', 'created_by', 'metadata', 'access_count', 'last_access', 'prev_access'):
                assert row[field] == original[field], ('repeat changed retained field', field)
    if supersedes:
        target = seed_ids[case.get('winner_target', 0)]
        assert supersedes[0]['memory_id'] == target
        detail = json.loads(supersedes[0]['detail'])
        assert detail['winner_id'] == primary and detail['rule'] == 'near_dup'
        if case.get('superseded', True):
            assert rows[target]['superseded_by'] == primary
            if not case.get('removed'):
                assert rows[target]['valid_to'] == rows[primary]['created_at']
    if 'recalled' in case:
        prior = {row['id']: row['access_count'] for row in before['memories']}
        assert sum(row['access_count'] - prior[row['id']] for row in state['memories'] if row['id'] in prior) == case['recalled']
        if 'recalled_target' in case:
            for index, identity in enumerate(seed_ids):
                assert rows[identity]['access_count'] - prior[identity] == int(index == case['recalled_target'])
    if case.get('superseded_row'):
        assert rows[seed_ids[0]]['superseded_by'] == primary
    return primary


def diagnostics(case, transcript, seed_ids, primary, interval, bindings):
    """Keep ordered literal payloads; bind only validated log times and IDs."""
    if case.get('stderr_pipe'):
        assert transcript['stderr'] is None  # Closed sink, not fabricated empty bytes.
        return None, []
    expected = []
    if case['outcome'] == 'repeat':
        expected.append(f'INFO conflict: duplicate write deduplicated id={primary}')
    if case['name'] in ('checker-error-after-admission', 'checker-recall-error-after-admission'):
        stage = 'duplicate lookup' if case['name'] == 'checker-error-after-admission' else 'candidate recall'
        detail = f"conflict: {stage}: invalid character 'b' looking for beginning of value"
        expected.append('WARN conflict check degraded; storing memory unchanged error=' + json.dumps(detail))
    if 'scan_index' in case:
        stage = 'duplicate lookup' if case['query'] == 'hello world' else 'candidate recall'
        detail = (f'conflict: {stage}: sql: Scan error on column index {case["scan_index"]}, '
                  'name "created_at": unsupported Scan, storing driver.Value type string into type *time.Time')
        expected.append('WARN conflict check degraded; storing memory unchanged error=' + json.dumps(detail))
    if case['name'] == 'supersede-error':
        expected.append(f'WARN failed to mark memory superseded loser={seed_ids[0]} error="constraint failed: supersession denied (1811)"')
    for name, action in [('pending-audit-error', 'conflict_pending'), ('supersede-audit-error', 'supersede')]:
        if case['name'] == name:
            expected.append(f'WARN failed to write {action} audit event error="constraint failed: audit denied (1811)"')
    raw = base64.b64decode(transcript['stderr']).decode('utf-8')
    lines = raw.splitlines(keepends=True)
    assert len(lines) == len(expected) + int(case.get('failure', False)), ('stderr line count', raw, expected)
    times, stable = [], []
    start, end = interval
    for index, payload in enumerate(expected):
        match = re.fullmatch(r'(\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2}) (.*)\n', lines[index])
        assert match and match[2] == payload, ('conflict diagnostic payload/order', lines[index], payload)
        parsed = datetime.datetime.strptime(match[1], '%Y/%m/%d %H:%M:%S')
        # Go LstdFlags and Chrono Local both use this host's local timezone.
        at = int(parsed.timestamp()) * 10**9
        assert start // 10**9 * 10**9 <= at <= end, ('log timestamp outside process interval', match[1], interval)
        times.append(dict(line=index, literal=match[1], ns=at))
        stable.append('$log_time ' + payload + '\n')
    if case.get('failure'):
        failure = f'symbrain memory set: store memory: memory not found: {primary}\n'
        assert lines[-1] == failure, ('governance diagnostic must follow conflict log', raw)
        stable.append(failure.replace(primary, bindings['ids'].get(primary, primary)))
    return ''.join(stable), times


def compare_pair(go, rust, root, env, path, args, case, seed_ids, handler, report, preparation, completed):
    """One actual Go/native pair; reused for the unchanged original boundary input."""
    saved = root / 'before.sqlite'
    with closing(sqlite3.connect(path)) as db, closing(sqlite3.connect(saved)) as backup:
        db.backup(backup)
    before, pair = writes.snapshot(path), []
    for binary in (go, rust):
        with closing(sqlite3.connect(saved)) as backup, closing(sqlite3.connect(path)) as db:
            backup.backup(db)
        handler.requests, handler.database, handler.mutate = [], path, case.get('on_request')
        start = time.time_ns()
        if case.get('stderr_pipe'):
            read_fd, write_fd = os.pipe()
            os.close(read_fd)
            with os.fdopen(write_fd, 'wb', buffering=0) as sink:
                result = subprocess.run([str(binary), 'memory', *args], cwd=root, env=env,
                                        stdout=subprocess.PIPE, stderr=sink, timeout=15)
            transcript = dict(exit=result.returncode, stdout=base64.b64encode(result.stdout).decode(), stderr=None)
        else:
            transcript = replay.output(binary, args, root, env)
        end = time.time_ns()
        state = writes.snapshot(path)
        raw = dict(transcript=transcript, state=state, interval_ns=[start, end], requests=list(handler.requests))
        pair.append(raw)
        try:
            primary = observe(case, state, before, transcript, seed_ids)
            for row in state['memories']:
                if row['id'] in seed_ids:
                    for field, value in case.get('mutated_fields', {}).items():
                        assert row[field] == value, ('post-admission input changed', field, row)
            stable, bindings = canonical(state, before, (start, end), primary, case.get('mutated_fields', {}))
            stderr, log_times = diagnostics(case, transcript, seed_ids, primary, (start, end), bindings)
            bindings['diagnostic_times'] = log_times
            expected_requests = [dict(path='/v1/embeddings', body=dict(model=case.get('model', 'owned-model'), input=[case['query']]))]
            assert raw['requests'] == expected_requests
            fts = writes.audit_fts(path)
            assert fts == sorted((row['id'], row['content'], row['scope']) for row in state['memories'])
            with closing(sqlite3.connect(path)) as db:
                schema = db.execute('SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name').fetchall()
            stdout = base64.b64decode(transcript['stdout'])
            if primary is not None:
                stdout = stdout.replace(primary.encode(), bindings['ids'].get(primary, primary).encode())
            raw.update(bindings=bindings, schema=schema, fts=fts, comparison=dict(
                exit=transcript['exit'], stdout=stdout.decode(), stderr=stderr,
                state=stable, requests=raw['requests'], schema=schema))
        except Exception as error:
            writes.save_json(report, dict(status='FAIL', case=case, args=args, error=str(error), before=before,
                                          preparation=preparation, processes=pair, completed=completed))
            raise
    record = dict(case=case, args=args, preparation=preparation, before=before, go=pair[0], rust=pair[1],
                  match=pair[0]['comparison'] == pair[1]['comparison'], native_fallback_available=False)
    # Save mismatches before failing, including the original boundary caller.
    writes.save_json(report, dict(status='RUNNING' if record['match'] else 'FAIL', cases=[*completed, record]))
    assert record['match'], 'actual Go/native conflict state differs'
    return record


def execute(go, rust, report):
    records = []
    server = http.server.HTTPServer(('127.0.0.1', 0), Embeddings)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for case in scenarios():
            with tempfile.TemporaryDirectory(prefix='memory-conflict-') as temporary:
                root = Path(temporary)
                env = writes.environment(root)
                env.update(SYMMEMORY_OLLAMA_URL=f'http://127.0.0.1:{server.server_port}/api/embeddings',
                           SYMMEMORY_OLLAMA_MODEL='owned-model', SYMMEMORY_CONFLICT_ENABLED='false')
                path = root / 'memory.db'
                Embeddings.mutate = None
                seed_ids, preparation = prepare(go, root, env, path, case)
                env.pop('SYMMEMORY_CONFLICT_ENABLED')
                env.update(case.get('env', {}))
                config = root / 'config/symmemory/config.toml'
                config.parent.mkdir(parents=True, exist_ok=True)
                config.write_text(case.get('config', ''), encoding='utf-8')
                args = ['set', case['query'], '--kind', 'user', '--db', str(path), '--output', case['format'],
                        '--scope', case.get('scope', 'global'), '--entities', case.get('entities', '')]
                if case.get('staged'):
                    args.append('--staged')
                records.append(compare_pair(go, rust, root, env, path, args, case, seed_ids,
                                            Embeddings, report, preparation, records))
                writes.save_json(report, dict(status='RUNNING', cases=records))
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    document = dict(go_revision=replay.ORACLE, go_binary_sha256=replay.digest(go.read_bytes()),
                    rust_binary_sha256=replay.digest(rust.read_bytes()), cases=records,
                    total=len(records), passed=sum(row['match'] for row in records), native_fallback_available=False,
                    unix_stderr_pipe_executed=os.name == 'posix')
    writes.save_json(report, document)
    assert document['total'] == len(scenarios()) > 0
    assert document['passed'] == document['total'], 'actual Go/native conflict state differs'


def boundaries(go, rust, wrapper, report):
    """Actual delegated outputs; none of these cases counts as native parity."""
    cases = [
        ('provider', {'SYMMEMORY_CONFLICT_LLM_PROVIDER': 'ollama'}, ''),
        ('provider-disabled', {'SYMMEMORY_CONFLICT_LLM_PROVIDER': 'ollama', 'SYMMEMORY_CONFLICT_ENABLED': 'false'}, ''),
        ('prefilter', {'SYMMEMORY_HYBRID_SEARCH_PREFILTER_ENABLED': 'true'}, ''),
        ('hex-threshold', {'SYMMEMORY_CONFLICT_NEAR_DUP_THRESHOLD': '0x1.ep-1'}, ''),
        ('nan-threshold', {'SYMMEMORY_CONFLICT_NEAR_DUP_THRESHOLD': 'NaN'}, ''),
        ('float-max', {}, '[conflict]\nmax_candidates=1.5\n'),
    ]
    records = []
    server = http.server.HTTPServer(('127.0.0.1', 0), Embeddings)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for name, overrides, config_text in cases:
            with tempfile.TemporaryDirectory(prefix='memory-conflict-boundary-') as temporary:
                root = Path(temporary)
                path, trace = root / 'memory.db', root / 'delegated.json'
                env = writes.environment(root)
                endpoint = f'http://127.0.0.1:{server.server_port}'
                env.update(SYMMEMORY_OLLAMA_URL=endpoint, SYMMEMORY_OLLAMA_MODEL='owned-model',
                           SYMMEMORY_CONFLICT_LLM_URL=endpoint, SYMMEMORY_CONFLICT_LLM_MODEL='owned-model')
                assert replay.output(go, ['list', '--db', str(path)], root, env)['exit'] == 0
                config = root / 'config/symmemory/config.toml'
                config.parent.mkdir(parents=True, exist_ok=True)
                config.write_text(config_text, encoding='utf-8')
                env.update(overrides)
                env.update(SYMBRAIN_GO_BINARY=str(wrapper), MEMORY_CLI_FALLBACK_GO=str(go), MEMORY_CLI_FALLBACK_RECEIPT=str(trace))
                Embeddings.requests, Embeddings.mutate = [], None
                before = writes.snapshot(path)
                output = replay.output(rust, ['set', 'hello world', '--kind', 'reference', '--db', str(path), '--json'], root, env)
                raw = dict(name=name, transcript=output, before=before, after=writes.snapshot(path), requests=list(Embeddings.requests))
                records.append(raw)
                writes.save_json(report, dict(cases=records))
                assert trace.is_file(), ('unsupported config was not delegated', name, output)
                delegated = json.loads(trace.read_text())
                raw['delegated'] = delegated
                assert output == {key: delegated[key] for key in ('exit', 'stdout', 'stderr')}
                assert raw['requests'] == [dict(path='/v1/embeddings', body=dict(model='owned-model', input=['hello world']))]
                raw['match'] = True
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
        writes.save_json(report, dict(cases=records, total=len(records), passed=sum(row.get('match', False) for row in records), native_parity_cases=0))
