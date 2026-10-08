import sys,json,tempfile,hashlib,subprocess
from pathlib import Path
from types import SimpleNamespace
from urllib.parse import urlencode
sys.path.insert(0,'/workspace/symaira-fetch773-proxy-auth/browse/port/harness')
import fetch_control_proxy_auth as auth,fetch_control_raw_proxy as raw,fetch_control_proxy_fixture as fixture
args=SimpleNamespace(go=Path('/tmp/symaira-fetch773-proxy-auth-clean-go-probe'),rust=Path('/workspace/symaira-fetch773-proxy-auth/target/debug/examples/control_probe'))
infos=[]
for start in range(0,256,32):
 encoded=''.join('%%%02X'%v for v in range(start,start+32))
 infos += [('octets-user-'+str(start),encoded+':owned'),('octets-password-'+str(start),'owned:'+encoded)]
infos += [('subdelimiters',"!$&'()*+,;=._~-:!$&'()*+,;=._~-"),('literal-at','first@second:third@fourth'),('first-colon','a:b:c:d'),('encoded-colon','a%3Ab:c%3Ad'),('nested-percent','%253A:%25ff'),('empty-both',':'),('empty-userinfo','')]
observed=[]
with tempfile.TemporaryDirectory(prefix='fetch773-independent-boundaries-') as tmp:
 root=Path(tmp);server,thread=fixture.start();proxy='http://127.0.0.1:'+str(server.server_port)
 try:
  selected=[]; expected=[]
  for label,info in infos:
   for port in ['080','81']:
    for caller in [None,'','synthetic-caller']:
     selected.append(dict(id=f'{label}-{port}-{repr(caller)}',url=f'http://93.184.216.34:{port}/echo',proxy=proxy.replace('://','://'+info+'@',1),headers={} if caller is None else {'Proxy-Authorization':caller}))
     expected.append(([caller] if caller is not None else [])+[auth.basic(info)])
  (root/'bytes').mkdir()
  results=raw.pairs(args,selected,root/'bytes')
  for row,headers in zip(results,expected):
   assert row['matched'],row
   assert json.loads(bytes.fromhex(row['rust']['body_hex']))['headers']['proxy-authorization']==headers,row
  observed+=results
  # Cross-host strips caller origin/proxy/cookie; selected proxy auth is freshly appended exactly once.
  for port in ['080','81']:
   target=f'http://owned-final.test:{port}/echo'
   middle='http://owned-middle.test:080/redirect?'+urlencode({'target':target})
   selected=[dict(id='three-hop-'+port,url='http://93.184.216.34:080/redirect?'+urlencode({'target':middle}),proxy=proxy.replace('://','://user:%ff@',1),headers={'Proxy-Authorization':'synthetic-caller','Authorization':'synthetic-origin','Cookie':'owned=yes'})]
   (root/('redirect'+port)).mkdir()
   results=raw.pairs(args,selected,root/('redirect'+port))
   for row in results:
    assert row['matched'],row
    headers=json.loads(bytes.fromhex(row['rust']['body_hex']))['headers']
    assert headers['proxy-authorization']==[auth.basic('user:%ff')] and 'authorization' not in headers and 'cookie' not in headers,row
   observed+=results
 finally:fixture.stop(server,thread)
report=dict(head=subprocess.check_output(['git','-C','/workspace/symaira-fetch773-proxy-auth','rev-parse','HEAD'],text=True).strip(),matched=len(observed),octets_covered=list(range(256)),pairs=observed,binaries_sha256={k:hashlib.sha256(p.read_bytes()).hexdigest() for k,p in vars(args).items()})
Path('/tmp/symaira-fetch773-proxy-auth-independent-boundaries.json').write_text(json.dumps(report,indent=2)+'\n');print('Independent auth boundaries',len(observed),'matched; all256 bytes in both fields, both routes, 3 caller states; two actual three-hop chains')
