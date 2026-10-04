from pathlib import Path
import os,json,hashlib,subprocess,threading,time,socket,http.client,base64,hmac,signal
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
binary=Path('/workspace/oracles/symbrain-go-dcddcef0');source=Path('/workspace/oracles/daemon772-go-source');repo=Path('/workspace/symaira-memory763-ui');root=Path('/tmp/symaira-memory763-ui-oracle-complete');root.mkdir(exist_ok=False)
for name in ['home','config','data','cache','cwd']: (root/name).mkdir()
secret='synthetic-memory763-owned-secret-32bytes'
env=dict(HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_CACHE_HOME=str(root/'cache'),PATH='',JWT_SECRET_KEY=secret)
cli=[];http_rows=[];embedding_requests=[]
class Embeddings(BaseHTTPRequestHandler):
 def do_POST(self):
  body=self.rfile.read(int(self.headers.get('Content-Length','0')));embedding_requests.append(dict(path=self.path,body_hex=body.hex()))
  response=json.dumps({'embedding':[0.25]*768}).encode();self.send_response_only(200);self.send_header('Content-Length',str(len(response)));self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(response)
 def log_message(self,*a):pass
stub=ThreadingHTTPServer(('127.0.0.1',0),Embeddings);thread=threading.Thread(target=stub.serve_forever,daemon=True);thread.start()
config=root/'config/symmemory';config.mkdir();(config/'config.toml').write_text('[ollama]\nurl = "http://127.0.0.1:'+str(stub.server_port)+'/api/embeddings"\nmodel = "synthetic-owned-768"\n[conflict]\nenabled = false\n')
commands=[['help'],['memory','--help'],['memory','serve','--help'],['memory','tui'],['memory','ui'],['memory','web'],['memory','dashboard'],['memory','--tui'],['tui'],['ui'],['web'],['dashboard'],['memory','serve','--port','0']]
for arguments in commands:
 p=subprocess.run([str(binary),*arguments],env=env,cwd=root/'cwd',capture_output=True,timeout=10)
 cli.append(dict(argv=arguments,exit=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex()))
# Allocate one disposable loopback port; the real CLI owns listener, config, SQLite, JWT and route stack.
s=socket.socket();s.bind(('127.0.0.1',0));port=s.getsockname()[1];s.close();db=root/'data/owned.sqlite'
stdout=open(root/'server.stdout','wb');stderr=open(root/'server.stderr','wb');p=subprocess.Popen([str(binary),'memory','serve','--port',str(port),'--db',str(db)],env=env,cwd=root/'cwd',stdout=stdout,stderr=stderr)
def request(name,path,method='GET',body=None,headers=None):
 conn=http.client.HTTPConnection('127.0.0.1',port,timeout=10);raw=json.dumps(body).encode() if body is not None else None;h=dict(headers or {})
 if raw is not None:h['Content-Type']='application/json'
 conn.request(method,path,raw,h);response=conn.getresponse();data=response.read();record=dict(id=name,method=method,path=path,headers=h,request_body_hex='' if raw is None else raw.hex(),status=response.status,response_headers=response.getheaders(),body_hex=data.hex());http_rows.append(record);conn.close();return record,data
encode=lambda b:base64.urlsafe_b64encode(b).rstrip(b'=');now=int(time.time());header=encode(b'{"alg":"HS256","typ":"JWT"}');payload=encode(json.dumps(dict(jti='owned-memory763-token',iss='symaira-memory',sub='owned-human',iat=now,exp=now+3600),separators=(',',':')).encode());unsigned=header+b'.'+payload;token=(unsigned+b'.'+encode(hmac.new(secret.encode(),unsigned,hashlib.sha256).digest())).decode();auth={'Authorization':'Bearer '+token}
try:
 deadline=time.monotonic()+15
 while time.monotonic()<deadline:
  if p.poll() is not None:raise AssertionError('actual Go CLI exited '+str(p.returncode))
  try:
   conn=socket.create_connection(('127.0.0.1',port),timeout=.1);conn.close();break
  except OSError:time.sleep(.03)
 else:raise AssertionError('actual CLI never listened')
 for path in ['/','/style.css','/app.js']:
  row,data=request('static-'+path,path);assert row['status']==200,row
  name='index.html' if path=='/' else path[1:];assert data==(source/'internal/memory/web/static'/name).read_bytes(),name
 for name,path,headers in [('static-host-denial','/',{'Host':'untrusted.invalid'}),('list-no-token','/api/list',{}),('list-invalid-token','/api/list',{'Authorization':'Bearer invalid'}),('status-no-token','/api/status',{}),('status-invalid-token','/api/status',{'Authorization':'Bearer invalid'})]:
  row,data=request(name,path,headers=headers);expected=403 if name=='static-host-denial' else 200 if name.startswith('status') else 401;assert row['status']==expected,row
 for name,path in [('list-owned-token','/api/list'),('rules-owned-token','/api/rules'),('entities-owned-token','/api/entities')]:
  row,data=request(name,path,headers=auth);assert row['status']==200,row
 row,data=request('ui-add-memory','/api/set','POST',dict(content='Synthetic memory763 blue fox agent context',scope='global',metadata={'owned':'memory763'}),auth);assert row['status']==200,row;identifier=json.loads(data)['id']
 row,data=request('ui-list-after-add','/api/list',headers=auth);assert row['status']==200 and any(m['id']==identifier for m in (json.loads(data) or [])),row
 row,data=request('ui-scope-filter','/api/list?scope=project',headers=auth);assert row['status']==200 and all(m['scope']=='project' for m in (json.loads(data) or [])),row
 row,data=request('ui-search','/api/search','POST',dict(query='Synthetic memory763 blue fox',scope='global'),auth);assert row['status']==200,row
 row,data=request('ui-delete-memory','/api/delete?id='+identifier,'DELETE',headers=auth);assert row['status']==200 and json.loads(data)['deleted'] is True,row
 row,data=request('ui-list-after-delete','/api/list',headers=auth);assert row['status']==200 and not any(m['id']==identifier for m in (json.loads(data) or [])),row
finally:
 (root/'partial-observations.json').write_text(json.dumps(dict(cli=cli,http=http_rows,embedding_requests=embedding_requests),indent=2)+'\n')
 if p.poll() is None:p.send_signal(signal.SIGTERM)
 try:p.wait(timeout=8)
 except subprocess.TimeoutExpired:p.kill();p.wait();raise
 stdout.close();stderr.close();stub.shutdown();stub.server_close();thread.join(timeout=3)
manifest={str(path.relative_to(source)):hashlib.sha256(path.read_bytes()).hexdigest() for path in [source/'cmd/symbrain/main.go',*source.glob('cmd/symbrain/cmd_memory*.go'),*source.glob('internal/memory/web/**/*'),source/'internal/memory/mcp/http_server.go',source/'internal/memory/mcp/handlers.go',source/'internal/memory/tui/dashboard.go'] if path.is_file()}
report=dict(candidate_base=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),frozen_go=subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip(),binary=str(binary),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),owned_root=str(root),cli=cli,http=http_rows,embedding_requests=embedding_requests,server_exit=p.returncode,source_manifest=manifest,limits='Actual Go CLI/HTTP wire reachability and API operations only; no browser or Rust/native parity claim; no TUI invoked because no reachable command exists; all credentials/data/network peers synthetic and owned.')
(root/'receipt.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(dict(cli=len(cli),http=len(http_rows),embedding_requests=len(embedding_requests),server_exit=p.returncode,root=str(root)),indent=2))
