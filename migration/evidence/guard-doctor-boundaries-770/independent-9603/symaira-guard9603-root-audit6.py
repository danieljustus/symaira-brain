from pathlib import Path
import sys,json,tempfile,time,subprocess,hashlib,os
sys.path.insert(0,'/workspace/symaira-guard770-doctor-boundaries/scripts/guard-standalone-oracle');import replay
GO=Path('/tmp/symaira-guard770-diagnostics-independent-go');RUST=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard');rows=[]
with tempfile.TemporaryDirectory(prefix='guard770-independent-auditfs-') as raw:
 root=Path(raw)
 for i,kind in enumerate(('normal','directory','parent-file','final-symlink','ancestor-symlink','unsafe-parent')):
  pair={}
  for side,binary,native in [('go',GO,False),('rust',RUST,True)]:
   owned=root/str(i)/side;env=replay.setup(owned,'empty');log=owned/'data/symguard/audit.log';sentinel=owned/'sentinel';sentinel.write_bytes(b'owned-original\n')
   if kind=='directory':log.mkdir(parents=True)
   if kind=='parent-file':log.parent.write_bytes(b'owned-parent-file')
   if kind=='final-symlink':log.parent.mkdir();log.symlink_to(sentinel)
   if kind=='ancestor-symlink':(owned/'sentinel-dir').mkdir();log.parent.symlink_to(owned/'sentinel-dir',target_is_directory=True)
   if kind=='unsafe-parent':(owned/'part').mkdir();env['XDG_DATA_HOME']=str(owned/'part/../data')
   start=time.time();p=subprocess.run([str(binary),'decide'],cwd=owned/'project',env=env,input=b'{"command":"owned","risk_class":"low"}',capture_output=True,timeout=5);end=time.time()
   response=json.loads(p.stdout);assert p.returncode==0 and not p.stderr
   if native:
    assert response['decision']==('allow' if kind=='normal' else 'deny')
    assert sentinel.read_bytes()==b'owned-original\n'
    if kind=='normal':assert len(log.read_bytes().splitlines())==1 and log.stat().st_mode&0o777==0o600 and log.parent.stat().st_mode&0o777==0o700
    elif kind=='ancestor-symlink':assert not list((owned/'sentinel-dir').iterdir())
    elif kind=='unsafe-parent':assert not log.exists()
   files={str(f.relative_to(owned)):dict(mode=f.lstat().st_mode&0o777,type='link' if f.is_symlink() else 'file',raw_hex=f.read_bytes().hex() if f.is_file() else None) for f in owned.rglob('*') if f.is_file() or f.is_symlink()}
   pair[side]=dict(exit=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex(),response=response,files=files)
  rows.append(dict(kind=kind,native_invariant_passed=True,**pair))
report=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd='/workspace/symaira-guard770-doctor-boundaries',text=True).strip(),cases=len(rows),results=rows,binaries={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (GO,RUST)},scope='Actual owned filesystem capability controls. Different legacy symlink/unsafe-parent behavior is recorded, not treated as equality or a new change; RawJsonlAppender remains unchanged.')
Path('/tmp/symaira-guard9603-root-audit-filesystem.json').write_text(json.dumps(report,indent=2)+'\n');print([(x['kind'],x['go']['response']['decision'],x['rust']['response']['decision']) for x in rows])
