from pathlib import Path
import sys,threading,tempfile,json,subprocess
sys.path.insert(0,'/workspace/symaira-fetch773-proxy-uri/browse/port/harness')
import fetch_control_process as h
from http.server import ThreadingHTTPServer,BaseHTTPRequestHandler
class Handler(BaseHTTPRequestHandler):
 def do_GET(self):
  body=json.dumps({'request_target':self.path,'host':self.headers.get('Host')},sort_keys=True,separators=(',',':')).encode()
  self.send_response_only(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
 def log_message(self,*_):pass
s=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=s.serve_forever,daemon=True).start()
p='http://127.0.0.1:'+str(s.server_port)
cases=[dict(id='proxy-target-port-'+port,url='http://93.184.216.34:'+port+'/path',method='GET',proxy=p) for port in ['80','080','00080','81','00081']]
try:
 with tempfile.TemporaryDirectory(prefix='fetch773-extra-proxy-') as raw:
  root=Path(raw);gs=h.execute(Path('/tmp/symaira-fetch773-second-fix-independent-go-probe'),cases,root/'go');rs=h.execute(Path('/workspace/symaira-fetch773-proxy-uri/target/debug/examples/control_probe'),cases,root/'rust')
 rows=[dict(id=c['id'],input=c,go=g,rust=r,equal=h.comparable(g)==h.comparable(r)) for c,g,r in zip(cases,gs,rs)]
 report=dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd='/workspace/symaira-fetch773-proxy-uri',text=True).strip(),observations=rows,passed=sum(r['equal'] for r in rows),failed=sum(not r['equal'] for r in rows))
 Path('/tmp/symaira-fetch773-proxy-uri-independent-original5.json').write_text(json.dumps(report,indent=2)+'\n')
 for r in rows:
  print(r['id'],r['equal'])
  for who in ('go','rust'): print(who,bytes.fromhex(r[who].get('body_hex','')).decode())
finally:s.shutdown();s.server_close()
