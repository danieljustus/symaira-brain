#!/usr/bin/env python3
"""Prove conservative routes actually delegate Go and preserve literal replies."""
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import tempfile
import replay
import writes


def execute(go,rust,wrapper,report):
    server=writes.http.server.HTTPServer(('127.0.0.1',0),writes.Embeddings)
    thread=writes.threading.Thread(target=server.serve_forever,daemon=True); thread.start()
    records=[]
    scenarios=[
        ('default-conflict','set','hello world',[],None),
        ('PII','set','contact fixture@example.test',['--staged'],None),
        ('extraction','set','I prefer tea.',['--staged'],None),
        ('Unicode-content','set','hello β',['--staged'],None),
        ('Unicode-metadata','set','hello world',['--staged','--metadata','{"a":"β"}'],None),
        ('invalid-duplicate-metadata','set','hello world',['--staged','--metadata','{"a":1,"a":"ok"}'],None),
        ('invalid-scope','set','hello world',['--staged','--scope','invalid'],None),
        ('new-file','set','hello world',['--staged'],None),
        ('bad-metadata-delete','delete','victim',[],("metadata",'{"a":1}')),
        ('bad-embedding-delete','delete','victim',[],("embedding",'invalid json')),
        ('bad-time-delete','delete','victim',[],("created_at",'invalid time')),
        ('alternate-time-delete','delete','victim',[],("created_at",'2000-01-01T00:00:00Z')),
    ]
    try:
        for name,verb,content,extra,mutation in scenarios:
            with tempfile.TemporaryDirectory(prefix='memory-write-boundary-') as temporary:
                root=Path(temporary); env=replay.isolated_env(root); path=root/'memory.db'; trace=root/'delegated.json'
                env.update(SYMMEMORY_OLLAMA_URL=f'http://127.0.0.1:{server.server_port}/api/embeddings',SYMMEMORY_CONFLICT_ENABLED='false')
                assert replay.output(go,['list','--db',str(path)],root,env)['exit']==0
                if name=='default-conflict': env.pop('SYMMEMORY_CONFLICT_ENABLED')
                if mutation:
                    with closing(sqlite3.connect(path)) as db, db:
                        db.execute("INSERT INTO memories(id,content,scope,kind,metadata,embedding,created_at,updated_at) VALUES('victim','hello world','global','reference','{}','[]','2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC')")
                        field,value=mutation
                        db.execute('UPDATE memories SET '+field+'=? WHERE id=\'victim\'',(value,))
                before=writes.snapshot(path)
                if name=='new-file': path=root/'new.db'
                args=[verb,content,'--db',str(path),'--json',*extra]
                if verb=='set': args+=['--kind','fact']
                env.update(SYMBRAIN_GO_BINARY=str(wrapper),MEMORY_CLI_FALLBACK_GO=str(go),MEMORY_CLI_FALLBACK_RECEIPT=str(trace))
                output=replay.output(rust,args,root,env)
                assert trace.is_file(), ('not delegated',name,output)
                delegated=json.loads(trace.read_text())
                assert output=={key:delegated[key] for key in ('exit','stdout','stderr')}, (name,output,delegated)
                after=writes.snapshot(path)
                unchanged=before==after
                if name in ('invalid-duplicate-metadata','invalid-scope','bad-metadata-delete','bad-embedding-delete','bad-time-delete'):
                    assert delegated['exit'] in (1,2) and unchanged, (name,delegated,unchanged)
                records.append(dict(name=name,match=True,transcript=output,delegated=delegated,database_before=before,database_after=after,database_unchanged=unchanged))
    finally:
        server.shutdown(); server.server_close(); thread.join()
    report.write_text(json.dumps(dict(cases=records,passed=len(records),total=len(records)),ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(dict(fallback_boundaries=len(records))))
