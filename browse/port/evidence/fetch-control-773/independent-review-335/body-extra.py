import sys,json,socketserver,threading,tempfile,hashlib,os,subprocess
from pathlib import Path
sys.path.insert(0,'/workspace/symaira-fetch773-proxy-uri/browse/port/harness')
import fetch_control_process as common
GO=Path('/tmp/symaira-fetch773-second-fix-independent-go-probe');RUST=Path('/workspace/symaira-fetch773-proxy-uri/target/debug/examples/control_probe')
payloads={
 'chunked-trailer':b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTrailer: X-Probe\r\n\r\n3\r\nabc\r\n0\r\nX-Probe: final\r\n\r\n',
 'short-chunk':b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nab',
 'short-length':b'HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nab',
 'extra-length':b'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nabc',
 'empty-length':b'HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\nabc',
 'no-content':b'HTTP/1.1 204 No Content\r\nContent-Length: 3\r\n\r\nabc',
 'early-hints':b'HTTP/1.1 103 Early Hints\r\nLink: <owned>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc',
 'duplicate-same-length':b'HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Length: 3\r\n\r\nabc',
 'duplicate-different-length':b'HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\nabc',
}
class H(socketserver.StreamRequestHandler):
 def handle(self):
  line=self.rfile.readline().decode();parts=line.split();path=parts[1].rsplit('/',1)[-1]
  while self.rfile.readline() not in (b'\r\n',b''):pass
  self.wfile.write(payloads[path]);self.wfile.flush()
class S(socketserver.ThreadingTCPServer):allow_reuse_address=True;daemon_threads=True
processes={}
def execute(binary,cases,root):
 root.mkdir();env={k:os.environ[k] for k in ('SystemRoot','WINDIR','TMP','TEMP') if k in os.environ}
 env.update(HOME=str(root),USERPROFILE=str(root),XDG_CONFIG_HOME=str(root/'config'),XDG_CACHE_HOME=str(root/'cache'),XDG_DATA_HOME=str(root/'data'),PATH='',SYMBROWSE_GO_BINARY=str(root/'absent-go'))
 p=subprocess.run([str(binary)],input=json.dumps(cases).encode(),capture_output=True,cwd=root,env=env,timeout=15)
 rows=[json.loads(line) for line in p.stdout.splitlines()]
 assert p.returncode==0 and [r['id'] for r in rows]==[c['id'] for c in cases]
 processes[str(binary)]=dict(exit=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex())
 return rows
s=S(('127.0.0.1',0),H);t=threading.Thread(target=s.serve_forever,daemon=True);t.start()
try:
 cases=[dict(id=kind+'-'+port,url='http://93.184.216.34:'+port+'/'+kind,proxy='http://127.0.0.1:'+str(s.server_address[1])) for kind in payloads for port in ('080','81')]
 with tempfile.TemporaryDirectory(prefix='fetch773-independent-body-') as tmp:
  root=Path(tmp);left=execute(GO,cases,root/'go');right=execute(RUST,cases,root/'rust')
 rows=[dict(case=c,go=g,rust=r,matched=common.comparable(g)==common.comparable(r)) for c,g,r in zip(cases,left,right)]
finally:s.shutdown();s.server_close();t.join(timeout=3)
report=dict(processes=processes,total=len(rows),passed=sum(x['matched'] for x in rows),pairs=rows,payloads={k:v.hex() for k,v in payloads.items()},binaries={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (GO,RUST)})
Path('/tmp/symaira-fetch773-proxy-uri-independent-body-extra.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
