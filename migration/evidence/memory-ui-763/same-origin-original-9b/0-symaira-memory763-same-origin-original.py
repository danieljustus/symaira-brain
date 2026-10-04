import sys,json,sqlite3,time
from pathlib import Path
sys.path.insert(0,'/workspace/symaira-memory763-ui/scripts/memory-http-oracle')
from support import Peer,Embeddings,GO,owned_env,seed,token,digest,snapshot
root=Path('/tmp/symaira-memory763-same-origin-source9b-original');root.mkdir(exist_ok=False)
native=Path('/workspace/symaira-memory763-ui/target/debug/examples/http_probe');embeddings=Embeddings();peers=[];rows=[]
try:
 owned=root/'seed';env=owned_env(owned,embeddings.url);database,seed_record=seed(owned,env)
 for is_native,name,binary in [(False,'go',GO),(True,'rust',native)]:
  owned=root/name;env=owned_env(owned,embeddings.url);current=owned/'current.db'
  with sqlite3.connect(database) as original,sqlite3.connect(current) as copy:original.backup(copy)
  peer=Peer(binary,owned,env,is_native,current);peers.append(peer)
  before=snapshot(current)
  row=peer.request('same-origin-post','/api/set','POST',{'content':'Azure fox specimen'},{'Authorization':'Bearer '+token(),'Origin':'http://127.0.0.1:'+str(peer.port)})
  rows.append(dict(owner=name,actual=row,state_unchanged=before==snapshot(current)))
  assert row['status']==403 and rows[-1]['state_unchanged']
finally:
 shutdown=[p.close() for p in peers];embeddings.close()
 (root/'receipt.json').write_text(json.dumps(dict(source='9b92e1ad6006585f3d5109b54fe500400043e50a',go_binary_sha256=digest(GO),native_binary_sha256=digest(native),actual_seed=seed_record,pairs=rows,shutdown=shutdown,limits='Actual manually supplied browser-equivalent same-Origin POST headers; not an actual graphical browser request or native write success.'),indent=2)+'\n')
print('Go/native same-origin authenticated POST actual403/no-write each')
