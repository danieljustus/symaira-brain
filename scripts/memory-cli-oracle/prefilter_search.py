"""Additive configured-search producer. SOURCE_ONLY until real processes run.

Called only by replay.py --family prefilter_search; its existing families and
counts are untouched. All stored vectors/LSH originate in the pinned Go writer.
Cloned rows and corruptions below are explicit disposable scenario inputs.
"""
import base64
from contextlib import closing
import copy
import datetime
import http.server
import json
from pathlib import Path
import sqlite3
import tempfile
import threading
import time

import replay
import writes

QUERY = 'Hamming search β'
PREFILTER = 'SYMMEMORY_HYBRID_SEARCH_PREFILTER_ENABLED'


def scenarios():
    cases = [
        dict(name='selected-feedback'),
        dict(name='disabled', env={PREFILTER: 'false'}),
        dict(name='nil-disables-whole-query', mutation="embedding_binary=NULL WHERE id='m-079'"),
        dict(name='empty-blob-not-nil', mutation="embedding_binary=X'' WHERE id='m-016'"),
        dict(name='short-blob', mutation="embedding_binary=zeroblob(7) WHERE id='m-016'"),
        dict(name='mismatched-95', mutation="embedding_binary=zeroblob(95) WHERE id='m-016'"),
        dict(name='mismatched-97', mutation="embedding_binary=zeroblob(97) WHERE id='m-016'"),
        dict(name='all-mismatched-ties', mutation='embedding_binary=zeroblob(7)', ties=True),
        dict(name='distance-and-original-index-order', ties=True),
        dict(name='empty-candidates', count=0),
        dict(name='scope', scope='user', mutation="scope='user' WHERE id >= 'm-016' AND id < 'm-026'"),
        dict(name='empty-scope-match', scope='missing'),
        dict(name='dimension-before-nil', mutation="embedding_dim=3,embedding_binary=NULL WHERE id='m-000'"),
        dict(name='hydrated-dimension', mutation="embedding='[1,2,3]' WHERE id='m-016'"),
        dict(name='hydrated-empty', mutation="embedding='[]' WHERE id='m-016'"),
        dict(name='other-source-before-nil', mutation="embedding_source='other',embedding_binary=NULL WHERE id='m-000'"),
        dict(name='quantization-before-nil', mutation="embedding_quantization='binary',embedding_binary=NULL WHERE id='m-000'"),
        dict(name='lsh-neighbor', bucket_xor=1),
        dict(name='lsh-outside', bucket_xor=7),
        dict(name='candidate-cap', count=2010, limit='9223372036854775807'),
        dict(name='scalar-filters', filters=True),
        dict(name='empty-cli-trust-policy-time-filters', mutation="valid_from='2090-01-01 00:00:00 +0000 UTC',valid_to='1999-01-01 00:00:00 +0000 UTC',superseded_by='old',metadata='{\"confidence\":\"low\",\"sensitivity\":\"secret\",\"sharing_level\":\"private\"}'"),
        dict(name='excluded-hydration-error', on_request="embedding='broken' WHERE id='m-000'"),
        dict(name='selected-hydration-error', on_request="embedding='broken' WHERE id='m-016'", error=True),
        dict(name='projected-metadata-before-selection', on_request="metadata='[]' WHERE id='m-000'", error=True),
        dict(name='projected-time-before-json', on_request="metadata='[]',created_at='invalid' WHERE id='m-000'", error=True),
        dict(name='scan-before-dimension-filter', on_request="embedding_dim=3,metadata='[]' WHERE id='m-000'", error=True),
        dict(name='feedback-best-effort', feedback=False),
        dict(name='global-enabled', global_config='[hybrid_search]\nprefilter_enabled=true\n', env={}),
        dict(name='project-enabled', global_config='[hybrid_search]\nprefilter_enabled=false\n', project='[hybrid_search]\nprefilter_enabled=true\n', env={}),
        dict(name='project-false-is-zero-skipped', global_config='[hybrid_search]\nprefilter_enabled=true\n', project='[hybrid_search]\nprefilter_enabled=false\n', env={}),
        dict(name='env-false-overrides-file-true', global_config='[hybrid_search]\nprefilter_enabled=true\n', env={PREFILTER: 'false'}),
        dict(name='env-true-overrides-files', project='[hybrid_search]\nprefilter_enabled=false\n'),
        dict(name='cli-keeps-default-weights', global_config='[ranking]\nspreading_weight=1\nrelevance_weight=0.1\nimportance_weight=0.9\n'),
        dict(name='mixed-signs', mixed=True),
        dict(name='controlled-hash-fallback', hash_fallback=True),
        dict(name='initial-malformed-still-delegated', mutation="embedding='broken' WHERE id='m-000'", delegated=True, error=True),
    ]
    cases.extend(dict(name='limit-' + limit, limit=limit) for limit in
                 ('0', '-1', '-9223372036854775808', '16', '17', '20', '2001',
                  '2305843009213693952', '4611686018427387921', '9223372036854775807'))
    return [dict(case, format=fmt) for case in cases for fmt in ('json', 'table')]


class Embeddings(writes.Embeddings):
    requests = []
    path = None
    case = {}

    def do_POST(self):
        cls = type(self)
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        cls.requests.append(dict(path=self.path, body=body))
        if cls.case.get('on_request'):
            with closing(sqlite3.connect(cls.path)) as db, db:
                db.execute('UPDATE memories SET ' + cls.case['on_request'])
        vector = [0.25, -0.25, 0.0, -0.0] * 192 if cls.case.get('mixed') else [0.25] * 768
        failed = cls.case.get('hash_fallback', False)
        raw = json.dumps({} if failed else {'data': [{'index': 0, 'embedding': vector}]}).encode()
        self.send_response(503 if failed else 200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def prepare(go, root, env, path, case, transcripts):
    def run(args):
        result = replay.output(go, [*args, '--db', str(path), '--json'], root, env)
        transcripts.append(dict(args=args, result=result))
        assert result['exit'] == 0, ('Go preparation failed', result)
        return json.loads(base64.b64decode(result['stdout']))
    run(['list'])
    identity = run(['set', QUERY, '--kind', 'reference'])['id']
    with closing(sqlite3.connect(path)) as db:
        db.row_factory = sqlite3.Row
        template = dict(db.execute('SELECT * FROM memories WHERE id=?', (identity,)).fetchone())
    run(['delete', identity])
    assert len(template['embedding_binary']) == 96 and template['embedding_dim'] == 768
    count = case.get('count', 80)
    with closing(sqlite3.connect(path)) as db, db:
        for index in range(count):
            row = dict(template)
            stamp = (datetime.datetime(2000, 1, 1) + datetime.timedelta(seconds=count-index)).strftime('%Y-%m-%d %H:%M:%S') + ' +0000 UTC'
            binary = bytearray(template['embedding_binary'])
            if index < 16:
                binary = bytearray(value ^ 255 for value in binary)
            else:
                binary[0] ^= (1 << ((index - 16) % 3)) - 1
            row.update(id=f'm-{index:03}', content=f'candidate {index}', metadata='{}',
                       created_at=stamp, updated_at=stamp, valid_from=None, valid_to=None,
                       last_access='1999-01-01 00:00:00 +0000 UTC', prev_access=None,
                       access_count=2, importance=0 if case.get('ties') else count-index,
                       embedding_binary=bytes(binary), lsh_hash=template['lsh_hash'] ^ case.get('bucket_xor', 0))
            names = ','.join('"' + name + '"' for name in row)
            db.execute(f'INSERT INTO memories({names}) VALUES({",".join("?" for _ in row)})', tuple(row.values()))
        if case.get('mutation'):
            db.execute('UPDATE memories SET ' + case['mutation'])
        if case.get('filters'):
            for index, mutation in enumerate(("consolidation_status='archived'", "review_status='staged'",
                    "retired_at='2000-01-01 00:00:00 +0000 UTC'", "tier='working',expires_at='2000-01-01 00:00:00 +0000 UTC'",
                    "embedding_source='other'", "embedding_quantization='binary'", 'embedding_dim=3', 'lsh_hash=-1')):
                db.execute(f"UPDATE memories SET {mutation},embedding_binary=NULL WHERE id=?", (f'm-{index:03}',))
        if not case.get('feedback', True):
            db.execute("CREATE TRIGGER fixture BEFORE UPDATE OF access_count ON memories BEGIN SELECT RAISE(ABORT,'feedback denied'); END")
    return dict(template_lsh=template['lsh_hash'], template_embedding_sha256=replay.digest(template['embedding'].encode()),
                template_binary_base64=base64.b64encode(template['embedding_binary']).decode(), cloned_input_rows=count)


def schema(path):
    with closing(sqlite3.connect(path)) as db:
        return db.execute('SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name').fetchall()


def restore(saved, path):
    with closing(sqlite3.connect(saved)) as source, closing(sqlite3.connect(path)) as target:
        source.backup(target)


def comparison(raw, before, case):
    """Full logical state; normalize only validated feedback/oplog clock values."""
    transcript, state = raw['transcript'], copy.deepcopy(raw['state'])
    stdout = base64.b64decode(transcript['stdout'])
    if case.get('error'):
        assert transcript['exit'] == 1 and not stdout and base64.b64decode(transcript['stderr'])
        ids = []
    else:
        assert transcript['exit'] == 0 and not base64.b64decode(transcript['stderr']), transcript
        if case['format'] == 'json':
            hits = json.loads(stdout)
            ids = [hit['memory']['id'] for hit in hits]
        else:
            lines = stdout.decode().splitlines()
            assert lines and (lines[0] == 'SCORE\tID\tSCOPE\tCREATED\tCONTENT' or lines == ['No relevant memories found.'])
            ids = [line.split('\t')[1] for line in lines[1:]]
        assert len(ids) == len(set(ids))
    old = {row['id']: row for row in before['memories']}
    assert set(old) == {row['id'] for row in state['memories']}
    start, end = raw['interval_ns']
    feedback = set(ids) if case.get('feedback', True) else set()
    times = []
    for row in state['memories']:
        previous = old[row['id']]
        selected = row['id'] in feedback
        assert row['access_count'] == previous['access_count'] + int(selected), ('wrong feedback set', row['id'])
        assert row['prev_access'] == (previous['last_access'] if selected else previous['prev_access'])
        if selected:
            at = writes.instant(row['last_access'])
            assert start <= at <= end, ('feedback timestamp', row)
            times.append(row['last_access'])
            row['last_access'] = '$feedback_time'
        else:
            assert row['last_access'] == previous['last_access']
    assert len(set(times)) <= 1, 'one batched feedback timestamp'
    old_events = {row['event_id'] for row in before['sync_oplog']}
    for event in state['sync_oplog']:
        if event['event_id'] not in old_events:
            assert start - 10**9 <= writes.instant(event['ts']) <= end
            event['ts'] = '$oplog_time'
    # Physical FTS encodings are retained in raw state and independently checked.
    # Logical FTS, schema/trigger text, every other table and every column compare.
    for name in list(state):
        if name.startswith('memories_fts_'):
            del state[name]
        else:
            state[name].sort(key=lambda row: json.dumps(row, sort_keys=True))
    return dict(transcript=transcript, state=state, schema=raw['schema'], fts=raw['fts'], requests=raw['requests'])


def execute(go, rust, report, records):
    assert report is not None, '--report is required to retain failures before assertions'
    assert __debug__, 'assertions are required for this producer'
    progress = dict(status='RUNNING', family='prefilter_search', records=records, controls=[],
                    go_revision=replay.ORACLE, go_binary_sha256=replay.digest(go.read_bytes()),
                    rust_binary_sha256=replay.digest(rust.read_bytes()), native_acceptance=False,
                    compiler_input_closure='REQUIRED at owning gate; binary hashes alone are not provenance')
    server = http.server.HTTPServer(('127.0.0.1', 0), Embeddings)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for index, case in enumerate(scenarios()):
            with tempfile.TemporaryDirectory(prefix='memory-prefilter-') as temporary:
                root = Path(temporary)
                # Reuse the producer's isolation then drop ambient secrets/proxies.
                env = replay.isolated_env(root)
                env = {key: value for key, value in env.items() if key in writes.environment(root)}
                for name in ('TMPDIR', 'TMP', 'TEMP'):
                    env[name] = str(root)
                env.update(SYMMEMORY_OLLAMA_URL=f'http://127.0.0.1:{server.server_port}/api/embeddings',
                           SYMMEMORY_OLLAMA_MODEL='owned-model', SYMMEMORY_CONFLICT_ENABLED='false',
                           SYMMEMORY_HYBRID_SEARCH_QUANTIZE_TO_BINARY='true')
                path, saved = root/'memory.db', root/'before.sqlite'
                Embeddings.path = path
                Embeddings.case = {key: case[key] for key in ('mixed', 'hash_fallback') if key in case}
                current = dict(family='prefilter_search', index=index, case=case, preparation=[], processes=[])
                progress['current'] = current
                current['inputs'] = prepare(go, root, env, path, case, current['preparation'])
                config = root/'config/symmemory/config.toml'
                config.parent.mkdir(parents=True, exist_ok=True)
                config.write_text(case.get('global_config', ''), encoding='utf-8')
                (root/'.symmemory.toml').write_text(case.get('project', ''), encoding='utf-8')
                env.update(case.get('env', {PREFILTER: 'true'}))
                current['before'] = writes.snapshot(path)
                current['before_schema'] = schema(path)
                current['before_raw_snapshot'] = replay.snapshot(root)
                with closing(sqlite3.connect(path)) as db, closing(sqlite3.connect(saved)) as backup:
                    db.backup(backup)
                args = ['search', QUERY, '--db', str(path), '--scope', case.get('scope', ''),
                        '--limit', case.get('limit', '5'), '--output', case['format']]
                current['args'] = args

                def capture(binary, process_env):
                    restore(saved, path)
                    assert writes.snapshot(path) == current['before'] and schema(path) == current['before_schema']
                    Embeddings.case, Embeddings.requests = case, []
                    start = time.time_ns()
                    transcript = replay.output(binary, args, root, process_env)
                    raw = dict(transcript=transcript, state=writes.snapshot(path), schema=schema(path),
                               interval_ns=[start, time.time_ns()], requests=list(Embeddings.requests),
                               raw_snapshot=replay.snapshot(root))
                    current['processes'].append(raw)
                    writes.save_json(report, progress)
                    raw['fts'] = writes.audit_fts(path)
                    assert raw['fts'] == sorted((row['id'], row['content'], row['scope']) for row in raw['state']['memories'])
                    assert raw['schema'] == current['before_schema']
                    return raw

                if case.get('delegated'):
                    absent = capture(rust, env)
                    assert absent['transcript']['exit'] == 1 and not base64.b64decode(absent['transcript']['stdout'])
                    assert base64.b64decode(absent['transcript']['stderr']).startswith(
                        f"symbrain: start Go fallback {root / 'absent-go'}: ".encode())
                    assert absent['state'] == current['before'] and not absent['requests']
                pair = []
                for binary in (go, rust):
                    process_env = dict(env)
                    if binary == rust and case.get('delegated'):
                        process_env['SYMBRAIN_GO_BINARY'] = str(go)
                    raw = capture(binary, process_env)
                    expected = [dict(path='/v1/embeddings', body=dict(model='owned-model', input=[QUERY]))]
                    assert raw['requests'] == expected, raw['requests']
                    raw['comparison'] = comparison(raw, current['before'], case)
                    pair.append(raw['comparison'])
                current.update(match=pair[0] == pair[1], fallback_available=case.get('delegated', False))
                records.append(current)
                writes.save_json(report, progress)
                assert current['match'], 'actual Go/native prefilter output or full state differs'
                if case['name'] == 'selected-feedback':
                    selected = [row['id'] for row in pair[0]['state']['memories'] if row['last_access'] == '$feedback_time']
                    assert len(current['before']['memories']) > 64 and len(selected) == 5
                    assert all(identity >= 'm-016' for identity in selected), 'selection must exclude better cosine/rank rows'
                    if case['format'] == 'json':
                        # A real candidate process with filtering disabled, not a
                        # fabricated Go result or a separate acceptance database.
                        mutant = capture(rust, dict(env, **{PREFILTER: 'false'}))
                        actual = comparison(mutant, current['before'], case)
                        rejected = pair[0] != actual
                        control = dict(name='filter-disabled-actual-output-and-feedback', rejected=rejected,
                                       baseline=pair[0], actual=actual)
                        progress['controls'].append(control)
                        writes.save_json(report, progress)
                        assert rejected and pair[0]['transcript'] != actual['transcript'] and pair[0]['state'] != actual['state'], 'prefilter negative control passed'
        assert len(records) == len(scenarios()) and len(progress['controls']) == 1
        progress.update(status='PASS_LOCAL_COMPARISON_ONLY', cases=len(records), passed=sum(row['match'] for row in records))
    except BaseException as error:
        progress.update(status='FAIL', error=f'{type(error).__name__}: {error}')
        raise
    finally:
        writes.save_json(report, progress)
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    return progress['controls']
