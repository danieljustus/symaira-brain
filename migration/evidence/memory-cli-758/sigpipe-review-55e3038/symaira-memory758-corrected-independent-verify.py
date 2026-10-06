import json,gzip,hashlib,subprocess,io,tarfile,pathlib
root=pathlib.Path('/workspace/symaira-memory758-write-fix'); h=lambda b:hashlib.sha256(b).hexdigest(); f=lambda p:h(pathlib.Path(p).read_bytes())
fix=json.load(open(root/'migration/evidence/memory-cli-758/governed-write-fix-647477f/verification.json'))
for p,x in fix['retained'].items():
 raw=(root/p).read_bytes();assert h(raw)==x['tracked_sha256'];data=gzip.decompress(raw) if p.endswith('.gz') else raw
 assert h(data)==x['original_sha256'] and len(data)==x['original_bytes']
for p,x in fix['binary_sha256'].items(): assert f(p)==x
old=json.load(open(root/'migration/evidence/memory-cli-758/governed-writes-14d2414/verification.json'))
for p,x in old['source_sha256'].items(): assert h(subprocess.check_output(['git','show',old['validated_clean_source']+':'+p],cwd=root))==x
for p,x in old['evidence'].items():
 raw=(root/'migration/evidence/memory-cli-758/governed-writes-14d2414'/p).read_bytes()
 assert h(raw)==x['stored_sha256']; data=gzip.decompress(raw) if p.endswith('.gz') else raw
 assert h(data)==x['original_sha256'] and len(data)==x['bytes']
pres=json.load(open(root/'migration/evidence/memory-cli-758/governed-write-review-28e8/preservation.json'))
for name,x in pres.items():
 if name=='reviewed-executable-archives.json': assert f(x['original'])==x['sha256']; continue
 raw=(root/x['tracked']).read_bytes(); assert h(raw)==x['tracked_sha256'];data=gzip.decompress(raw) if x['tracked'].endswith('.gz') else raw
 assert h(data)==x['original_sha256'];assert len(data)==x['original_bytes']
archives=json.load(open('/workspace/oracles/symaira-memory758-14d-binaries/receipt.json'))
for _,x in archives['files'].items():
 raw=pathlib.Path(x['archive']).read_bytes(); assert h(raw)==x['archive_sha256'];data=gzip.decompress(raw);assert h(data)==x['sha256'] and len(data)==x['bytes']
r=json.load(open('/tmp/symaira-memory758-corrected-independent-write-gate/receipt.json'));b=json.load(open('/tmp/symaira-memory758-corrected-independent-baseline.json'))
for d in[r,b]:
 assert d['candidate_revision']=='55e3038665946a9f077af335bec661d02191d004' and not d['candidate_dirty'];assert len(d['candidate_source_sha256'])==161
 assert all(f(root/p)==x for p,x in d['candidate_source_sha256'].items())
archive=subprocess.check_output(['git','archive',r['go_revision']],cwd=root)
with tarfile.open(fileobj=io.BytesIO(archive)) as t:
 assert len(r['frozen_go_source_sha256'])==1215
 for p,x in r['frozen_go_source_sha256'].items(): assert h(t.extractfile(p).read())==x
assert all(x['detected'] for x in r['controls'])
assert b['passed']==b['cases']==590
ctl=json.load(open('/tmp/symaira-memory758-corrected-independent-read-controls.json'));assert len(ctl['controls'])==2 and all(x['rejected_cases']==32 for x in ctl['controls'])
assert not subprocess.check_output(['git','diff','647477f','HEAD','--','rust','scripts','.github'],cwd=root).strip()
assert not subprocess.check_output(['git','diff','7ca3bed','HEAD','--','Cargo.toml','Cargo.lock','cmd','internal','fixtures'],cwd=root).strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root).strip()
print('PASS:26 corrected retained files;36 original evidence+36source;8 independent originals;6 archived binaries;8 current binaries/SDK;161 current source;1215 frozenGo;590baseline;2x32real controls; clean55source same647; no dependencies/Go/fixture edits')
