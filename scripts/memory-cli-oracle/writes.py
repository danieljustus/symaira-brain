#!/usr/bin/env python3
"""Real Go/native direct writes with validated UUID bindings and temporal relations."""
import argparse
import base64
import datetime
import hashlib
import http.server
import json
from contextlib import closing
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile
import threading
import time
import replay

UUID = re.compile(r"[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}\Z")
TIME = re.compile(r"(\d{4}-\d\d-\d\d)[ T](\d\d:\d\d:\d\d)(?:\.(\d+))?(?: \+0000 UTC|Z)?\Z")



def environment(root):
    # Runtime processes receive only platform necessities and owned control
    # wiring, never inherited provider credentials or proxy configuration.
    names = ('SystemRoot','SYSTEMROOT','WINDIR','COMSPEC','PATHEXT','TEMP','TMP','TMPDIR',
             'MEMORY_CLI_CONTROL_RUST','MEMORY_CLI_CONTROL_MODE',
             'MEMORY_CLI_FALLBACK_GO','MEMORY_CLI_FALLBACK_RECEIPT')
    env = {name: os.environ[name] for name in names if name in os.environ}
    env.update(HOME=str(root),USERPROFILE=str(root),XDG_CONFIG_HOME=str(root/'config'),
               XDG_DATA_HOME=str(root/'data'),XDG_CACHE_HOME=str(root/'cache'),
               XDG_STATE_HOME=str(root/'state'),PATH='',SYMBRAIN_GO_BINARY=str(root/'absent-go'))
    return env


def save_json(path, value):
    path.write_bytes((json.dumps(value,ensure_ascii=False,indent=2)+'\n').encode('utf-8'))

def instant(value):
    match = TIME.fullmatch(value)
    assert match, ("invalid time", value)
    seconds = datetime.datetime.fromisoformat(match[1] + "T" + match[2]).replace(tzinfo=datetime.timezone.utc).timestamp()
    return int(seconds) * 10**9 + int((match[3] or "").ljust(9, "0"))


def encode(value):
    if isinstance(value, bytes):
        return {"blob_base64": base64.b64encode(value).decode()}
    return value


def snapshot(path):
    with closing(sqlite3.connect(path)) as db, db:
        state = {}
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").fetchall():
            columns = [row[1] for row in db.execute('PRAGMA table_info("' + table + '")')]
            state[table] = [dict(zip(columns, [encode(value) for value in row]))
                            for row in db.execute('SELECT * FROM "' + table + '"')]
        return state


def audit_fts(path):
    # Shadows are physical index encoding, not independent application records.
    # Validate integrity and query the logical FTS row projection on both DBs.
    with closing(sqlite3.connect(path)) as db, db:
        db.execute("INSERT INTO memories_fts(memories_fts,rank) VALUES('integrity-check',1)")
        return db.execute("SELECT id,content,scope FROM memories_fts ORDER BY id").fetchall()


def canonical(state, before, interval, expected, returned_id=None, deleted=False):
    start, end = interval
    identities = {}
    generated = []
    primary = returned_id
    if primary:
        assert UUID.fullmatch(primary), ("primary UUID v4", primary)
        identities[primary] = "$memory"
        generated.append(primary)
    old_entities = {row['id'] for row in before['entities']}
    entities = [row for row in state['entities'] if row['id'] not in old_entities]
    for row in entities:
        assert UUID.fullmatch(row['id']), row
        identities[row['id']] = "$entity:" + row['name']
        generated.append(row['id'])
    old_audits = {row['id'] for row in before['audit_log']}
    audits = [row for row in state['audit_log'] if row['id'] not in old_audits]
    for index, row in enumerate(audits):
        assert UUID.fullmatch(row['id']), row
        identities[row['id']] = "$audit:" + str(index)
        generated.append(row['id'])
    assert len(generated) == len(set(generated)), "generated IDs must be distinct"
    time_receipt = []

    def timestamp(row, table, field, tolerance=0):
        value = row[field]
        at = instant(value)
        assert start - tolerance <= at <= end + tolerance, (table, field, value, interval)
        time_receipt.append(dict(table=table, id=row.get('id', row.get('event_id')), field=field, literal=value, ns=at))
        row[field] = '$time:' + table + ':' + field
        return at

    transformed = json.loads(json.dumps(state))
    memories = transformed['memories']
    if primary:
        rows = [row for row in memories if row['id'] == primary]
        assert len(rows) == 1, "one returned primary row"
        row = rows[0]
        valid = timestamp(row, 'memories', 'valid_from')
        created = timestamp(row, 'memories', 'created_at')
        updated = timestamp(row, 'memories', 'updated_at')
        assert valid <= created <= updated, "prepare/save/governance order"
        assert row['created_by'] == row['updated_by'] == expected['author']
        assert row['created_session'] == row['updated_session'] == ''
        assert row['kind'] == expected['kind'] and row['review_status'] == ('staged' if expected['staged'] else 'approved')
        assert row['importance'] == 0 and row['access_count'] == 1
        meta = json.loads(row['metadata'])
        if 'observed_at' not in expected['metadata']:
            observed = instant(meta['observed_at'])
            assert start // 10**9 * 10**9 <= observed <= end
            # Replace only this validated generated literal in the original JSON;
            # retain exact key order, escaping, every other key and every value.
            row['metadata'] = row['metadata'].replace('"observed_at":"' + meta['observed_at'] + '"', '"observed_at":"$observed"')
        set_events = [event for event in audits if event['action'] == 'set']
        assert len(set_events) == 1 and set_events[0]['memory_id'] == primary
        for field in ('valid_to', 'superseded_by', 'expires_at', 'last_access', 'prev_access', 'retired_at'):
            assert row[field] is None, (field, row[field])
    if deleted:
        assert all(row['id'] != expected['delete_id'] for row in memories)
        delete_events = [event for event in audits if event['action'] == 'delete']
        assert len(delete_events) == 1 and delete_events[0]['memory_id'] == expected['delete_id']
    for row in transformed['entities']:
        if row['id'] not in old_entities:
            created = timestamp(row, 'entities', 'created_at')
            updated = timestamp(row, 'entities', 'updated_at')
            assert created <= updated
    for row in transformed['audit_log']:
        if row['id'] not in old_audits:
            timestamp(row, 'audit_log', 'created_at')
    old_events = {row['event_id'] for row in before['sync_oplog']}
    for row in transformed['sync_oplog']:
        if row['event_id'] not in old_events:
            timestamp(row, 'sync_oplog', 'ts', 10**9)
    # Explicit ID substitution only in relationship columns, never arbitrary text.
    for table, rows in transformed.items():
        for row in rows:
            for field in ('id', 'memory_id', 'entity_id', 'target_id'):
                if isinstance(row.get(field), str) and row[field] in identities:
                    row[field] = identities[row[field]]
    shadows = [name for name in transformed if name.startswith('memories_fts_')]
    for name in shadows:
        transformed.pop(name)
    for rows in transformed.values():
        rows.sort(key=lambda row: json.dumps(row, sort_keys=True))
    return transformed, dict(ids=identities, times=time_receipt, physical_fts_shadow_sha256={
        name: replay.digest(json.dumps(state[name], sort_keys=True).encode()) for name in shadows})


class Embeddings(http.server.BaseHTTPRequestHandler):
    requests = []
    mode = 'success'
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.requests.append(dict(path=self.path, body=body))
        value = {'tiny': 1e-7, 'threshold': 1e-6, 'huge': 1e21, 'overflow': 1e39, 'negative-zero': -0.0}.get(self.mode, 0.25)
        vector = [value] * (767 if self.mode == 'wrong-dimension' else 768)
        if self.mode == 'mixed': vector = ([0.25, -0.25, 0.0, -0.0] * 192)
        data = {"data": [{"index": 0, "embedding": vector}]}
        if self.mode == 'extra-item': data['data'].append(data['data'][0])
        raw = json.dumps(data).encode() if self.mode != 'failure' else b'{}'
        self.send_response(503 if self.mode == 'failure' else 200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)
    def log_message(self, *args):
        pass


def scenarios():
    cases = []
    for kind in ('user', 'feedback', 'project', 'reference', 'fact', 'USER', 'user-pref', 'how-to', 'p r o j e c t'):
        for staged in (False, True):
            cases.append(dict(name=f'kind-{kind}-{staged}', kind=kind, staged=staged))
    for scope in ('', 'global', 'project', 'agent', 'user', 'session'):
        cases.append(dict(name='scope-' + scope, scope=scope, staged=True))
    for author in ('', 'cli:symbrain', 'Ada &<>\u2028\u2029', 'δοκιμή'):
        cases.append(dict(name='author-' + author, author=author, staged=True))
    for metadata in ('null', '{"a":null,"a":"last","source_type":"custom","observed_at":"2001-02-03T04:05:06Z"}',
                     '{"<&>\\u2028\\u2029":"value &<>","confidence":"low","source_uri":"literal"}', '   '):
        cases.append(dict(name='metadata-' + metadata, metadata_raw=metadata, staged=True))
    for entities in ('Alpha', 'alpha, ALT, New &<>\u2028\u2029, , New &<>\u2028\u2029', ',,', 'broken'):
        cases.append(dict(name='entities-' + entities, entities=entities, staged=True))
    cases.extend([
        *[dict(name='embedding-' + mode, mode=mode, staged=True) for mode in ('failure','tiny','threshold','huge','negative-zero','mixed','wrong-dimension','extra-item','overflow')],
        *[dict(name='quantized-' + mode, mode=mode, staged=True, quantized=True) for mode in ('success','mixed','failure')],
        dict(name='project-file-boundary', scope='project', project='[memory]\nstage_writes_by_default=true\n', staged=True),
        dict(name='project-git-file-boundary', scope='project', git_boundary=True, staged=True),
        dict(name='env-conflict-disable', conflict_env='false'),
        dict(name='table-approved',format='table'),
        dict(name='table-staged',format='table',staged=True),
        dict(name='repeat-disabled',repeat=True),
        dict(name='repeat-staged-bypasses-enabled-checker',repeat=True,staged=True,conflict_env='true'),
        dict(name='stage-config-is-not-cli-staged', global_config='[memory]\nstage_writes_by_default=true\n'),
        dict(name='null-after-value', metadata_raw='{"a":"first","a":null}', staged=True),
        dict(name='project-conflict-disable', global_config='[conflict]\nenabled=true\n', project='[conflict]\nenabled=false\n', conflict_env='false'),
        dict(name='ignored-file-false-default-true-staged', global_config='[conflict]\nenabled=false\n', staged=True, conflict_env=None),
        dict(name='invalid-config-falls-to-default-staged', global_config='[server]\nhttp_port="invalid"\n', staged=True, conflict_env=None),
    ])
    return cases


def execute(go, rust, report):
    records = []
    # Own the shipped default endpoint before any invalid-config reset can use it.
    default_server = http.server.HTTPServer(('127.0.0.1', 11434), Embeddings)
    default_thread = threading.Thread(target=default_server.serve_forever, daemon=True)
    default_thread.start()
    server = http.server.HTTPServer(('127.0.0.1', 0), Embeddings)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for case in scenarios():
            with tempfile.TemporaryDirectory(prefix='memory-writes-') as temporary:
                root = Path(temporary)
                env = environment(root)
                env['SYMMEMORY_OLLAMA_URL'] = f'http://127.0.0.1:{server.server_port}/api/embeddings'
                env['SYMMEMORY_OLLAMA_MODEL'] = 'owned-model'
                if case.get('quantized'): env['SYMMEMORY_HYBRID_SEARCH_QUANTIZE_TO_BINARY'] = 'true'
                if case.get('conflict_env', 'false') is not None:
                    env['SYMMEMORY_CONFLICT_ENABLED'] = case.get('conflict_env', 'false')
                config = root / 'config/symmemory/config.toml'
                config.parent.mkdir(parents=True)
                config.write_bytes(case.get('global_config', '').encode())
                if 'project' in case:
                    (root / '.symmemory.toml').write_bytes(case['project'].encode())
                if case.get('git_boundary'):
                    (root / '.git').touch()
                path = root / 'memory.db'
                assert replay.output(go, ['list', '--db', str(path)], root, env)['exit'] == 0
                with closing(sqlite3.connect(path)) as db, db:
                    db.execute("INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES('existing','Alpha','other','[null,\"ALT\"]','retained','original','2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC')")
                    db.execute("INSERT INTO entities_aliases(entity_id,alias) VALUES('existing','ALT')")
                    db.execute("INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES('malformed','broken','other','bad json','','original','2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC')")
                if case.get('repeat'):
                    seed_env=dict(env,SYMMEMORY_CONFLICT_ENABLED='false')
                    assert replay.output(go,['set','hello world','--kind','fact','--db',str(path)],root,seed_env)['exit']==0
                saved = root / 'before.sqlite'
                with closing(sqlite3.connect(path)) as source, closing(sqlite3.connect(saved)) as backup:
                    source.backup(backup)
                before = snapshot(path)
                args = ['set', '  hello world  ', '--kind', case.get('kind', 'fact'), '--db', str(path), '--author', case.get('author', 'cli:symbrain')]
                if case.get('format','json')=='json': args.append('--json')
                if case.get('staged'): args.append('--staged')
                args += ['--scope', case.get('scope', 'global'), '--metadata', case.get('metadata_raw', '{}'), '--entities', case.get('entities', '')]
                expected = dict(author=case.get('author', 'cli:symbrain'), kind={'fact':'reference','USER':'user','user-pref':'user','how-to':'reference','p r o j e c t':'project'}.get(case.get('kind'), case.get('kind', 'reference')), staged=case.get('staged',False), metadata=json.loads(case.get('metadata_raw','{}').strip() or '{}') or {})
                pair = []
                for binary in (go, rust):
                    with closing(sqlite3.connect(saved)) as backup, closing(sqlite3.connect(path)) as destination:
                        backup.backup(destination)
                    Embeddings.requests = []
                    Embeddings.mode = case.get('mode', 'success')
                    start = time.time_ns()
                    transcript = replay.output(binary, args, root, env)
                    end = time.time_ns()
                    state = snapshot(path)
                    try:
                        assert transcript['exit'] == 0, ('native set exit must be zero',case,binary,transcript)
                        raw_output=base64.b64decode(transcript['stdout'])
                        if case.get('format','json')=='json':
                            identity = json.loads(raw_output)['id']
                        else:
                            identity = re.search(rb'Memory ([a-f0-9-]{36}) \(',raw_output)[1].decode('ascii')
                        stable, binding = canonical(state, before, (start,end), expected, identity)
                    except Exception as error:
                        save_json(report,dict(failed_case=case,error=str(error),transcript=transcript,state=state,other_processes=pair,
                                              interval_ns=[start,end],go_binary_sha256=replay.digest(go.read_bytes()),rust_binary_sha256=replay.digest(rust.read_bytes())))
                        raise
                    stdout = base64.b64decode(transcript['stdout']).replace(identity.encode(), b'$memory')
                    assert identity.encode() in base64.b64decode(transcript['stdout'])
                    requests = list(Embeddings.requests)
                    model = 'nomic-embed-text' if 'invalid-config' in case['name'] else 'owned-model'
                    assert requests == [dict(path='/v1/embeddings',body=dict(model=model,input=['hello world']))], (case, requests)
                    fts = audit_fts(path)
                    assert fts == sorted((row['id'],row['content'],row['scope']) for row in state['memories'])
                    pair.append(dict(transcript=transcript, comparison=dict(exit=transcript['exit'],stderr=transcript['stderr'],stdout=base64.b64encode(stdout).decode(),state=stable, requests=requests), state=state, bindings=binding))
                records.append(dict(case=case, match=pair[0]['comparison']==pair[1]['comparison'], go=pair[0], rust=pair[1]))
    finally:
        server.shutdown(); server.server_close(); thread.join()
        default_server.shutdown(); default_server.server_close(); default_thread.join()
    document = dict(go_binary_sha256=replay.digest(go.read_bytes()),rust_binary_sha256=replay.digest(rust.read_bytes()), cases=records, passed=sum(row['match'] for row in records), total=len(records))
    save_json(report,document)
    assert document['passed']==document['total'], ('actual Go/native write state differs', [(row['case']['name'],row['match']) for row in records if not row['match']])
    print(json.dumps(dict(passed=document['passed'], total=document['total'])))


if __name__ == '__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--go',type=Path,required=True); parser.add_argument('--rust',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args()
    execute(args.go.resolve(),args.rust.resolve(),args.report)
