from pathlib import Path
import json,gzip,hashlib,subprocess,re,tarfile,io
r=Path('/workspace/symaira-memory758-sigpipe');h=lambda b:hashlib.sha256(b).hexdigest();f=lambda p:h(Path(p).read_bytes())
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()=='1a60e98a6847902299448f92f30ccee82e59913b'
assert not subprocess.check_output(['git','status','--porcelain'],cwd=r).strip()
v=json.loads((r/'migration/evidence/memory-cli-758/sigpipe-fix-458e8fa/verification.json').read_text())
for p,x in v['retained'].items():
 raw=(r/p).read_bytes();assert h(raw)==x['tracked_sha256'];data=gzip.decompress(raw) if p.endswith('.gz') else raw;assert h(data)==x['original_sha256'] and len(data)==x['original_bytes']
assert len(v['retained'])==36
for p,x in v['binary_sha256'].items():assert f(p)==x
old=json.loads((r/'migration/evidence/memory-cli-758/sigpipe-review-55e3038/retention.json').read_text())
for _,x in old['files'].items():
 raw=(r/x['tracked']).read_bytes();assert h(raw)==x['tracked_sha256'];data=gzip.decompress(raw) if x['tracked'].endswith('.gz') else raw;assert h(data)==x['original_sha256'] and len(data)==x['original_bytes']
assert len(old['files'])==46
for name in ['symaira-memory758-647-binaries','symaira-memory758-14d-binaries']:
 archives=json.loads((Path('/workspace/oracles')/name/'receipt.json').read_text())
 for _,x in archives['files'].items():
  raw=Path(x['archive']).read_bytes();assert h(raw)==x['archive_sha256'];data=gzip.decompress(raw);assert h(data)==x.get('original_sha256',x.get('sha256')) and len(data)==x.get('original_bytes',x.get('bytes'))
w=json.loads(Path('/tmp/symaira-memory758-458-root-writes/receipt.json').read_text());b=json.loads(Path('/tmp/symaira-memory758-458-root-baseline.json').read_text())
for d in [w,b]:
 assert d['candidate_revision']=='1a60e98a6847902299448f92f30ccee82e59913b' and not d['candidate_dirty'];assert len(d['candidate_source_sha256'])==163
 for p,x in d['candidate_source_sha256'].items():assert f(r/p)==x
archive=subprocess.check_output(['git','archive',w['go_revision']],cwd=r)
with tarfile.open(fileobj=io.BytesIO(archive)) as t:
 assert len(w['frozen_go_source_sha256'])==1215
 for p,x in w['frozen_go_source_sha256'].items():assert h(t.extractfile(p).read())==x
assert all(x['detected'] for x in w['controls']) and len(w['controls'])==3
for name,n in [('writes',60),('deletes',16),('fallback-boundaries',13),('write-failures',10)]:
 d=json.loads(Path('/tmp/symaira-memory758-458-root-writes',name+'.json').read_text());assert d['passed']==d['total']==n
assert b['passed']==b['cases']==590
ctl=json.loads(Path('/tmp/symaira-memory758-458-root-controls.json').read_text());assert len(ctl['controls'])==2 and all(x['rejected_cases']==32 for x in ctl['controls'])
extra=json.loads(Path('/tmp/symaira-memory758-458-root-original-closed-pipe.json').read_text());assert len(extra['records'])==2 and all(x['match'] and x['committed_state_matches'] and x['go']['transcript']['exit']==-13 and x['rust']['transcript']['exit']==-13 for x in extra['records'])
log=Path('/tmp/symaira-memory758-458-root-tests.log').read_text();counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log);assert sum(int(x[0]) for x in counts)==320 and sum(int(x[1])+int(x[2]) for x in counts)==0 and len(counts)==32
assert not subprocess.check_output(['git','diff','458e8fa5dc752ee2eed6446cc279649ad3d2b879','HEAD','--','rust','scripts','.github'],cwd=r).strip()
assert not subprocess.check_output(['git','diff','e3dbda6c','HEAD','--','Cargo.toml','Cargo.lock','cmd','internal','fixtures'],cwd=r).strip()
print('PASS: immutable1a60/source458;320tests/32summaries;60Set16Delete13boundaries10failurepairs;590baseline2x32controls3writecontrols;original2quietSIGPIPE+committedFTS;163candidate/1215frozenGo;36author/46originalartifacts;7actualcurrent+all647/14darchivebinaries;unchangedpins/frozenGo')
