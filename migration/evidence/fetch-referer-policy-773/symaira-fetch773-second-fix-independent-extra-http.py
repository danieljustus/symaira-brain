from pathlib import Path
import sys, threading,tempfile,json,hashlib,subprocess
sys.path.insert(0,'/workspace/symaira-fetch773/browse/port/harness')
import fetch_control_process as h
from urllib.parse import urlencode
from http.server import ThreadingHTTPServer
server=ThreadingHTTPServer(('127.0.0.1',0),h.Handler)
th=threading.Thread(target=server.serve_forever,daemon=True);th.start()
base='http://127.0.0.1:'+str(server.server_port)
cases=[]
def add(name,target='/echo-authority',start_fragment='',**kw):
 url=base+'/redirect-authority?'+urlencode({'target':target})+start_fragment
 cases.append(dict(id=name,url=url,method='GET',**kw))
add('fragment-automatic-referer',start_fragment='#private-fragment')
add('fragment-explicit-referer',headers={'Referer':'http://example.com/custom#fragment'})
add('userinfo-same-host');cases[-1]['url']=cases[-1]['url'].replace('http://','http://user:secret@')
add('userinfo-cross-host',target=base.replace('127.0.0.1','localhost')+'/echo-authority');cases[-1]['url']=cases[-1]['url'].replace('http://','http://user:secret@')
add('redirect-host-header',headers={'Host':'custom.example'})
cases.append(dict(id='direct-host-header',url=base+'/echo-authority',method='GET',headers={'Host':'custom.example'}))
add('head-to-get302');cases[-1]['method']='HEAD'
add('lowercase-method302');cases[-1]['method']='head'
try:
 with tempfile.TemporaryDirectory(prefix='fetch773-independent-extra-') as raw:
  root=Path(raw)
  go=h.execute(Path('/tmp/symaira-fetch773-second-fix-independent-go-probe'),cases,root/'go')
  rust=h.execute(Path('/workspace/symaira-fetch773/target/debug/examples/control_probe'),cases,root/'rust')
 rows=[dict(id=c['id'],input=c,go=g,rust=r,equal=h.comparable(g)==h.comparable(r)) for c,g,r in zip(cases,go,rust)]
 report=dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd='/workspace/symaira-fetch773',text=True).strip(),candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd='/workspace/symaira-fetch773')),observations=rows,passed=sum(r['equal'] for r in rows),failed=sum(not r['equal'] for r in rows))
 p=Path('/tmp/symaira-fetch773-second-fix-independent-extra-http.json');p.write_text(json.dumps(report,indent=2)+'\n')
 for r in rows:
  print(r['id'],r['equal']);
  if not r['equal']:
   for who in ('go','rust'):
    x=r[who];print(who,json.dumps(x));print(who+' decoded',bytes.fromhex(x.get('body_hex','')).decode(errors='replace'))
finally:
 server.shutdown();server.server_close()
