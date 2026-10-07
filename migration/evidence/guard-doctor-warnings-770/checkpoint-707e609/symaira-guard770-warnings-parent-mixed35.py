from pathlib import Path
import sys,tempfile,json,hashlib,subprocess
r=Path('/workspace/symaira-guard770-doctor-warnings');sys.path.insert(0,str(r/'scripts/guard-standalone-oracle'));import raw_paths
GO=Path('/tmp/symaira-guard770-warnings-final-process-binaries/go');RUST=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard')
components=[b'owned-\xff'+ '<>&\u2028\u2029\ufffd'.encode(),b'owned-\xf4\x90\x80\x80',b'owned-\xe2'+ '\u2028'.encode()+b'\x82',b'owned-\xf0\x9f\x92',b'owned-\x01\x7f'+ '\u0085\u200b'.encode()]
rows=[]
with tempfile.TemporaryDirectory(prefix='guard806-root-mixed-') as tmp:
 for i,(kind,part) in enumerate((k,p) for k in raw_paths.KINDS for p in components):
  root=Path(tmp)/str(i);left=raw_paths.observe(GO,kind,part,root/'go',False);right=raw_paths.observe(RUST,kind,part,root/'rust',True);disposition=raw_paths.compare(kind,left,right);assert disposition=='matched';rows.append(dict(kind=kind,component_hex=part.hex(),disposition=disposition,go=left,rust=right))
report={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip(),'candidate_clean':not subprocess.check_output(['git','status','--porcelain'],cwd=r).strip(),'total':len(rows),'matched':len(rows),'binary_sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [GO,RUST]},'results':rows}
Path('/tmp/symaira-guard770-warnings-parent-mixed35.json').write_text(json.dumps(report,indent=2)+'\n');print(len(rows),'actual mixed malformed-byte/HTML/JS/control pairs match; full stdout/stderr/exit/state')
