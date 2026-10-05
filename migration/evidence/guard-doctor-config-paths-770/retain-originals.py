import gzip,hashlib,json,os
from pathlib import Path
repo=Path('/workspace/symaira-guard770-config-paths')
out=repo/'migration/evidence/guard-doctor-config-paths-770/independent-4d78';out.mkdir(parents=True,exist_ok=False)
receipt=Path('/tmp/symaira-guard770-warnings-independent-review-receipt.json');r=json.loads(receipt.read_bytes());records=[]
def retain(path,expected=None):
 p=Path(path);raw=p.read_bytes();sha=hashlib.sha256(raw).hexdigest()
 if expected: assert sha==expected['sha256'] and len(raw)==expected['bytes'],path
 dest=out/(p.name+'.gz'); packed=gzip.compress(raw,mtime=0);dest.write_bytes(packed);assert gzip.decompress(dest.read_bytes())==raw
 records.append(dict(original=str(p),retained=dest.name,sha256=sha,bytes=len(raw),gzip_sha256=hashlib.sha256(packed).hexdigest()))
for p,m in r['evidence'].items():retain(p,m)
assert len(records)==92
retain(receipt)
elf=Path('/workspace/oracles/symaira-guard770-warnings-independent-4d-binaries/receipt.json');e=json.loads(elf.read_bytes());retain(elf)
assert len(e['records'])==69 and len(e['unique'])==46
for sha,m in e['unique'].items():
 packed=Path(m['archive']).read_bytes();raw=gzip.decompress(packed);assert len(raw)==m['bytes'] and hashlib.sha256(raw).hexdigest()==sha and hashlib.sha256(packed).hexdigest()==m['archive_sha256']
for m in e['records']:
 p=Path(m['path']);raw=p.read_bytes();assert len(raw)==m['bytes'] and hashlib.sha256(raw).hexdigest()==m['sha256']
states=json.load(open('/tmp/symaira-guard770-warnings-independent-owner-state-manifest-final.json'));assert len(states)==180
srecords=[];blobs=out/'state-blobs';blobs.mkdir()
for m in states:
 p=Path(m['path']);st=p.lstat();row=dict(m)
 if m['kind']=='symlink':assert os.readlink(p)==m['target']
 else:
  raw=p.read_bytes();sha=hashlib.sha256(raw).hexdigest();assert sha==m['sha256'] and len(raw)==m['bytes'] and st.st_mode==m['mode'] and st.st_mtime_ns==m['mtime_ns']
  b=blobs/(sha+'.gz');b.write_bytes(gzip.compress(raw,mtime=0));assert gzip.decompress(b.read_bytes())==raw;row['blob']='state-blobs/'+b.name
 srecords.append(row)
(out/'raw-state-retention.json').write_text(json.dumps(srecords,indent=2)+'\n')
(out/'retention.json').write_text(json.dumps(dict(original_review_head=r['head'],original_source=r['validated_source'],review_files=92,extra_original_receipts=2,records=records,raw_state_paths=180,elf_paths=69,unique_elf_bytes=46,elf_archive=str(elf),all_original_bytes_verified=True,note='Originals unchanged; existing ELF gzip archive verified before target reuse; no new execution claims.'),indent=2)+'\n')
print('PASS original92 + receipts2 + rawstate180 + ELF69/46')
