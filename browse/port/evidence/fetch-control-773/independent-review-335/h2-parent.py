import sys,json,tempfile,subprocess,hashlib
from pathlib import Path
sys.path.insert(0,'/workspace/symaira-fetch773-proxy-uri/browse/port/harness')
import fetch_control_process as common
import fetch_control_tls_fixture as tls
GO=Path('/tmp/symaira-fetch773-second-fix-independent-go-probe')
RUST=Path('/tmp/symaira-fetch773-522-parent-control-probe')
with tempfile.TemporaryDirectory(prefix='fetch773-independent-h2-') as tmp:
 root=Path(tmp);contexts,ca=tls.contexts(root/'tls')
 peer=subprocess.Popen(['/tmp/symaira-fetch773-proxy-uri-independent-h2-peer',str(root/'tls/valid.pem'),str(root/'tls/valid.key')],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
 try:
  addr=peer.stdout.readline().strip();assert addr
  cases=[dict(id='h2-https-proxy-'+port,url='http://93.184.216.34:'+port+'/echo',proxy='https://'+addr) for port in ('080','80','81')]
  left=common.execute(GO,cases,root/'go',tls.linux_trust_env(ca));right=common.execute(RUST,cases,root/'rust',tls.linux_trust_env(ca))
 finally:
  peer.terminate();peer.wait(timeout=3);stderr=peer.stderr.read()
 report=dict(pairs=[dict(case=c,go=g,rust=r,matched=common.comparable(g)==common.comparable(r)) for c,g,r in zip(cases,left,right)],peer_stderr=stderr,binaries={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (GO,RUST)},peer_source_sha256=hashlib.sha256(Path('/tmp/symaira-fetch773-proxy-uri-independent-h2-peer.go').read_bytes()).hexdigest())
 Path('/tmp/symaira-fetch773-proxy-uri-independent-h2-parent.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
