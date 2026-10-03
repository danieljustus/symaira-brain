import sys, os, json, tempfile, sqlite3, hashlib, subprocess, threading, time, base64
from pathlib import Path
from contextlib import closing
sys.path.insert(0,'/workspace/symaira-memory758-write-fix/scripts/memory-cli-oracle')
import writes, replay
ROOT=Path('/workspace/symaira-memory758-write-fix')
GO=Path('/workspace/oracles/symbrain-go-dcddcef0')
RUST=Path('/workspace/symaira-memory758-writes/target/debug/symbrain')
server=writes.http.server.HTTPServer(('127.0.0.1',0),writes.Embeddings)
thread=threading.Thread(target=server.serve_forever,daemon=True); thread.start()
records=[]
scenarios=[('new-metadata-json-output-closed-pipe','set',None,True),('new-metadata-table-output-closed-pipe','set-table',None,True)]
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
    if verb.startswith('set'): args+=['--kind','fact','--staged','--metadata','{"fixture":"value"}']
    args+=['--output','table' if verb.endswith('table') else 'json']
    start = time.time_ns()
    if full:
     read_fd, write_fd = os.pipe()
     os.close(read_fd)
     with os.fdopen(write_fd,'wb',buffering=0) as sink:
      result=subprocess.run(args,cwd=root,env=env,stdout=sink,stderr=subprocess.PIPE,timeout=12)
     transcript=dict(exit=result.returncode,stdout=None,stderr=base64.b64encode(result.stderr).decode())
    else:
     result=subprocess.run(args,cwd=root,env=env,capture_output=True,timeout=12)
     transcript=dict(exit=result.returncode,stdout=base64.b64encode(result.stdout).decode(),stderr=base64.b64encode(result.stderr).decode())
    end = time.time_ns()
    state=writes.snapshot(path)
    assert len(state['memories']) == 1
    identity = state['memories'][0]['id']
    stable, bindings = writes.canonical(state,before,(start,end),dict(author='cli:symbrain',kind='reference',staged=True,metadata={'fixture':'value'}),identity)
    try: fts=dict(ok=True,rows=writes.audit_fts(path))
    except Exception as error: fts=dict(ok=False,error=str(error))
    pair.append(dict(binary=str(binary),transcript=transcript,state=state,fts=fts,interval_ns=[start,end],bindings=bindings,canonical_state=stable))
   assert pair[0]['canonical_state'] == pair[1]['canonical_state'], 'all committed application state must match'
   assert pair[0]['fts']['ok'] and pair[1]['fts']['ok']
   records.append(dict(name=name,args=args[1:],trigger=trigger,before=before,go=pair[0],rust=pair[1],committed_state_matches=True,match=pair[0]['transcript']==pair[1]['transcript']))
finally:
 server.shutdown();server.server_close();thread.join()
receipt=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).decode().strip(),binary_sha256={str(x):hashlib.sha256(x.read_bytes()).hexdigest() for x in (GO,RUST)},records=records)
Path('/tmp/symaira-memory758-corrected-independent-closed-pipe.json').write_bytes((json.dumps(receipt,ensure_ascii=False,indent=2)+'\n').encode())
print(json.dumps([dict(name=x['name'],go_exit=x['go']['transcript']['exit'],rust_exit=x['rust']['transcript']['exit'],go_stderr=base64.b64decode(x['go']['transcript']['stderr']).decode(errors='replace'),rust_stderr=base64.b64decode(x['rust']['transcript']['stderr']).decode(errors='replace'),go_fts=x['go']['fts']['ok'],rust_fts=x['rust']['fts']['ok']) for x in records],ensure_ascii=False))
