#!/usr/bin/env python3
"""Actual CLI delete proofs: retained rows, cascades, audits and access-trigger oplog."""
import argparse
import base64
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import time
import replay
import writes


def execute(go, rust, report):
    cases=[]
    for scope,author in [('global','cli:symbrain'),('project','Ada &<>\u2028\u2029'),('agent',''),('user','δοκιμή'),('session','agent')]:
        for output in ['table','json']:
            cases.append((scope,author,output,'victim','ordinary'))
    cases.extend([('global','actor','json','missing','ordinary'),('global','actor','table',' missing ','ordinary')])
    for behavior in ('identity-after-access','disappears-after-access'):
        for output in ('table','json'): cases.append(('global','original',output,'victim',behavior))
    records=[]
    for scope,author,format,deleted_id,behavior in cases:
        with tempfile.TemporaryDirectory(prefix='memory-deletes-') as temporary:
            root=Path(temporary); env=writes.environment(root); path=root/'memory.db'
            assert replay.output(go,['list','--db',str(path)],root,env)['exit']==0
            with closing(sqlite3.connect(path)) as db, db:
                for identity in ('victim','retained'):
                    db.execute("INSERT INTO memories(id,content,scope,kind,metadata,embedding,created_at,updated_at,created_by,updated_by,created_session,updated_session,last_access,prev_access,access_count) VALUES(?,? ,?,'reference','{}','[]','2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC',?,?,'seed-session','seed-session','2002-01-01 00:00:00 +0000 UTC','2001-01-01 00:00:00 +0000 UTC',7)",(identity,'seed '+identity,scope,author,author))
                db.execute("INSERT INTO entities(id,name,type,aliases,description,created_at,updated_at) VALUES('e','shared','other','[]','','2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC')")
                db.executemany("INSERT INTO memory_entities(memory_id,entity_id) VALUES(?,'e')", [('victim',),('retained',)])
                db.executemany("INSERT INTO memory_evidence(id,memory_id,source_id,source_kind,text,evidence_text,char_start,char_end,alignment_status,created_at) VALUES(?,?,'seed','fixture','fact','fact',0,4,'exact','2000-01-01 00:00:00 +0000 UTC')", [('ev','victim'),('er','retained')])
                db.execute("INSERT INTO memory_associations(from_id,to_id,weight,created_at) VALUES('victim','retained',0.4,'2000-01-01 00:00:00 +0000 UTC')")
                if behavior=='identity-after-access':
                    db.execute("CREATE TRIGGER fixture_identity BEFORE UPDATE OF last_access ON memories BEGIN UPDATE memories SET created_by='post-access &<>',created_session='post-session' WHERE id=new.id; END")
                elif behavior=='disappears-after-access':
                    db.execute("CREATE TRIGGER fixture_delete BEFORE UPDATE OF last_access ON memories BEGIN DELETE FROM memories WHERE id=new.id; END")
            saved=root/'before.sqlite'
            with closing(sqlite3.connect(path)) as db, closing(sqlite3.connect(saved)) as backup: db.backup(backup)
            before=writes.snapshot(path); pair=[]
            for binary in (go,rust):
                with closing(sqlite3.connect(saved)) as backup, closing(sqlite3.connect(path)) as db: backup.backup(db)
                start=time.time_ns()
                transcript=replay.output(binary,['delete',deleted_id,'--db',str(path),'--output',format],root,env)
                end=time.time_ns(); state=writes.snapshot(path)
                writes.save_json(report,dict(behavior=behavior,scope=scope,author=author,format=format,id=deleted_id,transcript=transcript,state=state,other_processes=pair,
                                             go_binary_sha256=replay.digest(go.read_bytes()),rust_binary_sha256=replay.digest(rust.read_bytes())))
                success=deleted_id=='victim'
                assert transcript['exit']==(0 if success else 1), transcript
                try:
                    stable,binding=writes.canonical(state,before,(start,end),dict(delete_id=deleted_id,delete_audit=behavior!='disappears-after-access'),deleted=success)
                except Exception as error:
                    writes.save_json(report,dict(behavior=behavior,scope=scope,author=author,format=format,id=deleted_id,error=str(error),transcript=transcript,state=state,other_processes=pair,
                                                 go_binary_sha256=replay.digest(go.read_bytes()),rust_binary_sha256=replay.digest(rust.read_bytes())))
                    raise
                if success:
                    audits=[row for row in state['audit_log'] if row['action']=='delete']
                    if behavior=='disappears-after-access': assert audits==[]
                    else:
                        actual_scope,actual_author,actual_session=(scope,'post-access &<>','post-session') if behavior=='identity-after-access' else (scope,author or None,'seed-session')
                        assert len(audits)==1 and audits[0]['scope']==actual_scope and audits[0]['actor']==actual_author and audits[0]['session']==actual_session
                    assert state['memories']==[row for row in before['memories'] if row['id']=='retained']
                    assert state['memory_entities']==before['memory_entities']
                    assert state['memory_evidence']==[row for row in before['memory_evidence'] if row['memory_id']=='retained']
                    assert state['entities']==before['entities'] and state['memory_associations']==before['memory_associations']
                    events=[row['op'] for row in state['sync_oplog']]
                    expected_events={'ordinary':['upsert','upsert','upsert','delete'], 'identity-after-access':['upsert','upsert','upsert','upsert','delete'], 'disappears-after-access':['upsert','upsert','delete']}[behavior]
                    assert events==expected_events, (behavior,events)
                else:
                    assert state==before, 'missing delete must leave every table and column unchanged'
                fts=writes.audit_fts(path)
                expected=[('retained','seed retained',scope)] if success else [('retained','seed retained',scope),('victim','seed victim',scope)]
                assert fts==expected
                pair.append(dict(transcript=transcript,state=state,bindings=binding,comparison=dict(transcript=transcript,state=stable)))
            records.append(dict(behavior=behavior,scope=scope,author=author,format=format,id=deleted_id,go=pair[0],rust=pair[1],match=pair[0]['comparison']==pair[1]['comparison']))
    result=dict(go_binary_sha256=replay.digest(go.read_bytes()),rust_binary_sha256=replay.digest(rust.read_bytes()),cases=records,passed=sum(row['match'] for row in records),total=len(records))
    writes.save_json(report,result)
    assert result['passed']==result['total'], [row for row in records if not row['match']]
    print(json.dumps(dict(passed=result['passed'],total=result['total'])))


if __name__=='__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('--go',type=Path,required=True); parser.add_argument('--rust',type=Path,required=True); parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args(); execute(args.go.resolve(),args.rust.resolve(),args.report)
