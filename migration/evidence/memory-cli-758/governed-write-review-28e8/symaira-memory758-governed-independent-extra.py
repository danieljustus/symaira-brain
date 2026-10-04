import sys, os, json, tempfile, sqlite3, hashlib, subprocess, threading, time, base64
from pathlib import Path
from contextlib import closing
sys.path.insert(0,'/workspace/symaira-memory758-writes/scripts/memory-cli-oracle')
import writes, replay
ROOT=Path('/workspace/symaira-memory758-writes')
GO=Path('/workspace/oracles/symbrain-go-dcddcef0')
RUST=ROOT/'target/debug/symbrain'
server=writes.http.server.HTTPServer(('127.0.0.1',0),writes.Embeddings)
thread=threading.Thread(target=server.serve_forever,daemon=True); thread.start()
records=[]
scenarios=[
 ('kind-ignored','set',"CREATE TRIGGER fixture_kind BEFORE UPDATE OF kind ON memories BEGIN SELECT RAISE(IGNORE); END",False),
 ('stage-ignored','set',"CREATE TRIGGER fixture_stage BEFORE UPDATE OF review_status ON memories BEGIN SELECT RAISE(IGNORE); END",False),
 ('removed-in-set-audit','set',"CREATE TRIGGER fixture_audit AFTER INSERT ON audit_log WHEN new.action='set' BEGIN DELETE FROM memories WHERE id=new.memory_id; END",False),
 ('set-json-output-full','set',None,True),
 ('set-table-output-full','set-table',None,True),
 ('delete-json-output-full','delete',None,True),
 ('delete-table-output-full','delete-table',None,True),
]
try:
 for name,verb,trigger,full in scenarios:
  with tempfile.TemporaryDirectory(prefix='memory-independent-extra-') as temporary:
   root=Path(temporary); env=writes.environment(root)
   env.update(SYMMEMORY_CONFLICT_ENABLED='false',SYMMEMORY_OLLAMA_URL=f'http://127.0.0.1:{server.server_port}/api/embeddings')
   path=root/'memory.db'
   assert replay.output(GO,['list','--db',str(path)],root,env)['exit']==0
   if verb.startswith('delete'):
    with closing(sqlite3.connect(path)) as db,db:
     db.execute("INSERT INTO memories(id,content,scope,kind,metadata,embedding,created_at,updated_at) VALUES('victim','hello world','global','reference','{}','[]','2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC')")
   if trigger:
    with closing(sqlite3.connect(path)) as db,db: db.execute(trigger)
   saved=root/'saved.sqlite'
   with closing(sqlite3.connect(path)) as db, closing(sqlite3.connect(saved)) as backup: db.backup(backup)
   before=writes.snapshot(path); pair=[]
   for binary in (GO,RUST):
    with closing(sqlite3.connect(saved)) as backup,closing(sqlite3.connect(path)) as db: backup.backup(db)
    args=[str(binary),'memory',verb.split('-')[0],'victim' if verb.startswith('delete') else 'hello world','--db',str(path)]
    if verb.startswith('set'): args+=['--kind','fact','--staged']
    args+=['--output','table' if verb.endswith('table') else 'json']
    if full:
     with open('/dev/full','wb',buffering=0) as sink:
      result=subprocess.run(args,cwd=root,env=env,stdout=sink,stderr=subprocess.PIPE,timeout=12)
     transcript=dict(exit=result.returncode,stdout=None,stderr=base64.b64encode(result.stderr).decode())
    else:
     result=subprocess.run(args,cwd=root,env=env,capture_output=True,timeout=12)
     transcript=dict(exit=result.returncode,stdout=base64.b64encode(result.stdout).decode(),stderr=base64.b64encode(result.stderr).decode())
    state=writes.snapshot(path)
    try: fts=dict(ok=True,rows=writes.audit_fts(path))
    except Exception as error: fts=dict(ok=False,error=str(error))
    pair.append(dict(binary=str(binary),transcript=transcript,state=state,fts=fts))
   records.append(dict(name=name,args=args[1:],trigger=trigger,before=before,go=pair[0],rust=pair[1],match=pair[0]['transcript']['exit']==pair[1]['transcript']['exit']))
finally:
 server.shutdown();server.server_close();thread.join()
receipt=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).decode().strip(),binary_sha256={str(x):hashlib.sha256(x.read_bytes()).hexdigest() for x in (GO,RUST)},records=records)
Path('/tmp/symaira-memory758-governed-independent-extra.json').write_bytes((json.dumps(receipt,ensure_ascii=False,indent=2)+'\n').encode())
print(json.dumps([dict(name=x['name'],go_exit=x['go']['transcript']['exit'],rust_exit=x['rust']['transcript']['exit'],go_stderr=base64.b64decode(x['go']['transcript']['stderr']).decode(errors='replace'),rust_stderr=base64.b64decode(x['rust']['transcript']['stderr']).decode(errors='replace'),go_fts=x['go']['fts']['ok'],rust_fts=x['rust']['fts']['ok']) for x in records],ensure_ascii=False))
