import sys,json,os,subprocess,tempfile,time,shutil,hashlib
from pathlib import Path
ROOT=Path('/workspace/symaira-daemon772-state-key');sys.path.insert(0,str(ROOT/'browse/port/harness'));import daemon_state_key as gate
api=json.loads(Path('/tmp/symaira-state-key-clean-gate.json').read_text())['oracle_api']; rows=[]
for layout in ['forged-none','authenticated-nonjson']:
 row={'input':layout}
 for label,binary in [('go',Path('/tmp/symaira-pr801-go-1.26.7')),('rust',Path('/workspace/symaira-daemon772-registry/target/debug/symbrowse'))]:
  with tempfile.TemporaryDirectory(prefix='bk-show-') as t:
   root=Path(t);env=gate.registry.environment(root);env.update(SYMBROWSE_ENCRYPTION_KEY=gate.KEY,SYMBROWSE_NO_AUTOSTART='1');states=Path(env['XDG_STATE_HOME'])/'symbrowse/states';states.mkdir(parents=True,mode=0o700)
   if layout=='forged-none':
    raw=(gate.FROZEN/'encrypted-v3.state').read_bytes();magic=b'SYMBROWSE-STATE\x00';header,body=raw[len(magic):].split(b'\n',1);head=json.loads(header);head['key_source']='none';raw=magic+json.dumps(head,separators=(',',':')).encode()+b'\n'+body
   else:raw=bytes.fromhex(api['authenticated_nonjson_hex'])
   file=states/'malformed.json';file.write_bytes(raw);session='ks'+str(os.getpid());child,endpoint=gate.registry.start(binary,root,env,session)
   try:
    frame={'cmd':'state.show','session':session,'args':{'name':'malformed'}};response=gate.registry.harness.request(endpoint,frame);gate.registry.harness.request(endpoint,{'cmd':'daemon.stop','session':session});stdout,stderr=child.communicate(timeout=10);assert file.read_bytes()==raw
    row[label]={'root':str(root),'frame':frame,'response':response,'exit':child.returncode,'stdout':stdout.decode(),'stderr':stderr.decode(),'input_hex':raw.hex(),'binary_sha256':gate.registry.process.digest(binary)}
   finally:gate.registry.harness.kill_tree(child)
 row['matches']=row['go']['response']==row['rust']['response'];rows.append(row)
 print(layout,row['matches'],row['go']['response'],row['rust']['response'],flush=True)
Path('/tmp/symaira-state-key-f1-extra-show.json').write_text(json.dumps({'source_head':subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip(),'cases':rows},indent=2)+'\n')
