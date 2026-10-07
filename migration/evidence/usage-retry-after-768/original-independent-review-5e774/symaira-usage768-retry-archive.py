import gzip,hashlib,json,os,shutil
from pathlib import Path
TARGET=Path('/workspace/symaira-usage768-remaining/target'); ROOT=Path('/workspace/oracles/symaira-usage768-retry-original-review'); ROOT.mkdir(exist_ok=True)
oldfile=Path('/workspace/oracles/symaira-usage768-next-c0ae71-binaries/receipt.json');old=json.loads(oldfile.read_text());known={r['sha256']:r for r in old['records']};users=[]
for p in Path('/proc').iterdir():
 if not p.name.isdigit():continue
 for link in [p/'exe',p/'cwd',*list((p/'fd').glob('*'))]:
  try:s=os.readlink(link)
  except OSError:continue
  if s==str(TARGET)or s.startswith(str(TARGET)+'/'):users.append(dict(pid=p.name,path=str(link),value=s))
assert not users,users
files=[]
for p in TARGET.rglob('*'):
 if p.is_file():
  with p.open('rb') as f:magic=f.read(4)
  if magic==b'\x7fELF':files.append(p)
proof=Path('/tmp/symaira-usage768-next-independent-verification.json');v=json.loads(proof.read_text());proof_files=[Path(p) for p in v['raw_proof_sha256']]+[proof,Path('/tmp/symaira-usage768-next-independent-review-receipt.json')];elf_extras=[]
for p in proof_files:
 with p.open('rb')as f:magic=f.read(4)
 if magic==b'\x7fELF':elf_extras.append(p)
files=sorted(set(files+elf_extras));records=[];unique={};added=[];reused=[]
for p in files:
 data=p.read_bytes();h=hashlib.sha256(data).hexdigest()
 if h not in unique:
  if h in known:archive=Path(known[h]['archive']);reused.append(h)
  else:
   archive=ROOT/'elf-sha256'/(h+'.gz');archive.parent.mkdir(exist_ok=True)
   if not archive.exists():archive.write_bytes(gzip.compress(data,compresslevel=1,mtime=0))
   added.append(h)
  raw=gzip.decompress(archive.read_bytes());assert raw==data and hashlib.sha256(raw).hexdigest()==h
  unique[h]=dict(bytes=len(data),archive=str(archive),compressed_sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
 records.append(dict(original=str(p),sha256=h,bytes=len(data),archive=unique[h]['archive'],reused_author_archive=h in known))
# Preserve every reviewer raw proof: archive non-binary text/data losslessly;
# package tar.gz payloads retain verified raw content via owned hard links.
raw_records=[]
for p in sorted(set(proof_files)):
 data=p.read_bytes();h=hashlib.sha256(data).hexdigest();expected=v['raw_proof_sha256'].get(str(p));assert expected is None or expected==h,(p,h,expected)
 if data.startswith(b'\x7fELF'):
  archive=Path(unique[h]['archive']);kind='elf-gzip'
 elif p.name.endswith('.tar.gz'):
  archive=ROOT/'package-sha256'/(h+'.tar.gz');archive.parent.mkdir(exist_ok=True)
  if not archive.exists():
   try:os.link(p,archive)
   except OSError:
    assert shutil.disk_usage(ROOT).free>700000000+len(data)
    shutil.copyfile(p,archive)
  assert archive.read_bytes()==data;kind='package-tar-gz-raw-retained'
 else:
  archive=ROOT/'proof-sha256'/(h+'.gz');archive.parent.mkdir(exist_ok=True)
  if not archive.exists():archive.write_bytes(gzip.compress(data,compresslevel=1,mtime=0))
  assert gzip.decompress(archive.read_bytes())==data;kind='proof-gzip'
 raw_records.append(dict(original=str(p),sha256=h,bytes=len(data),archive=str(archive),storage=kind,archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest(),roundtrip_verified=True))
rlib=Path('/tmp/symaira-usage768-next-independent-retry-rlib.rlib');assert any(r['original']==str(rlib)for r in raw_records)
receipt=dict(source='c0ae71c0bc0a5e39c65458484f1024d5940a19aa',publication='5e77418ff03491df61a74dcd6435eeb5b4496260',target=str(TARGET),target_users=users,target_elf_paths=len([p for p in files if p.is_relative_to(TARGET)]),extra_actual_elf_paths=len(elf_extras),elf_paths=len(records),elf_unique=len(unique),reused_unique=len(reused),added_unique=len(added),new_compressed_elf_bytes=sum(Path(unique[h]['archive']).stat().st_size for h in added),records=records,unique=unique,raw_reviewer_proof_files=len(raw_records),original244_proofs=len(v['raw_proof_sha256']),proof_records=raw_records,original_author_archive=dict(path=str(oldfile),sha256=hashlib.sha256(oldfile.read_bytes()).hexdigest(),records=len(old['records']),unique=old['unique']),deleted=False,scope='All current target ELF artifacts including fresh review variants plus actual copied review executables/caller; all original244 raw proof bindings plus final verification and signed review receipt preserved. Reuse verified author gzip SHA payloads where identical; new payloads gzip/length/SHA roundtrip. Archival collection does not claim every historical dependency artifact came from candidate source. No source/target/proof deletion.')
(ROOT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({k:v for k,v in receipt.items()if k not in ['records','unique','proof_records']}))
