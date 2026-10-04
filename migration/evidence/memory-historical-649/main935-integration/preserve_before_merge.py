"""Lossless source-review preservation and read-only input admission before merge."""
import gzip,hashlib,io,json,os
from pathlib import Path
import subprocess,tarfile
BASE=Path('/workspace/review-proof/memory803-main935-integration')
REVIEW=Path('/workspace/review-artifacts/memory803-c3ccc-portable-review-doctor')
REPO=Path('/workspace/symaira-memory803-portable-admission-fixture')
receipt=REVIEW/'review-receipt.json';raw=receipt.read_bytes()
assert hashlib.sha256(raw).hexdigest()=='853e42bf9aa286a2d6736d0d74426d8dfb551c58e654f2a476b174a39d2837ad'
r=json.loads(raw);own=r['files']+[{'path':str(receipt),'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}]
assert len(own)==159
physical={};gitrows=[]
def record(path,expected,size=None):
 p=Path(path);s=p.lstat();h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
 assert h.hexdigest()==expected,str(p)
 if size is not None:assert s.st_size==size
 row={'original_path':str(p),'bytes':s.st_size,'sha256':expected,'mode':s.st_mode,'uid':s.st_uid,'gid':s.st_gid,'mtime_ns':s.st_mtime_ns,'atime_ns':s.st_atime_ns,'inode':s.st_ino,'device':s.st_dev,'preservation':'original-path-retained-unchanged'}
 physical[str(p)]=row;return row
for row in own:record(row['path'],row['sha256'],row['bytes'])
for row in r['prior_own_files']:record(row['path'],row['sha256'],row['bytes'])
record('/workspace/review-artifacts/memory803-a5c65-provider-review-doctor/review-receipt.json','ea69d2662b52180555a2379d470dc33ed986fe54c81649335160b14d858f5960')
for path,expected in r['prior_input_bindings'].items():
 if path.startswith('/'):record(path,expected)
 else:
  b=subprocess.check_output(['git','-C',str(REPO),'show',path]);assert hashlib.sha256(b).hexdigest()==expected
  ref,name=path.split(':',1);meta=subprocess.check_output(['git','-C',str(REPO),'ls-tree',ref,'--',name],text=True).split('\t')[0]
  gitrows.append({'git_reference':path,'bytes':len(b),'sha256':expected,'tree_entry':meta,'preservation':'immutable-Git-object-and-ref'})
archive=BASE/'complete-current-review.tar.gz'
with tarfile.open(archive,'w:gz',format=tarfile.PAX_FORMAT) as t:
 for row in own:
  p=Path(row['path']);native=physical[str(p)];info=tarfile.TarInfo(p.relative_to(REVIEW).as_posix());info.mode=native['mode']&0o7777;info.uid=native['uid'];info.gid=native['gid'];info.size=native['bytes'];info.mtime=native['mtime_ns']/1e9;info.pax_headers['mtime']=f"{native['mtime_ns']//10**9}.{native['mtime_ns']%10**9:09d}";t.addfile(info,io.BytesIO(p.read_bytes()));native['review_tar_member']=info.name
with tarfile.open(archive,'r:gz') as t:
 assert len(t.getmembers())==159
 for row in own:
  m=t.getmember(Path(row['path']).relative_to(REVIEW).as_posix());b=t.extractfile(m).read();assert len(b)==row['bytes'] and hashlib.sha256(b).hexdigest()==row['sha256'];n=physical[row['path']];assert (m.mode,m.uid,m.gid)==(n['mode']&0o7777,n['uid'],n['gid']);assert m.pax_headers['mtime']==f"{n['mtime_ns']//10**9}.{n['mtime_ns']%10**9:09d}"
result={'status':'complete-premerge-source-review-and-prior-inputs-retained','source':r['source'],'own_review_files':158,'own_receipt_extra':1,'prior_own_files':58,'prior_own_receipt_extra':1,'prior_input_bindings':422,'physical_records':list(physical.values()),'immutable_Git_records':gitrows,'full_current_review_archive':str(archive),'full_current_review_archive_members':159,'full_current_review_archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest(),'large_original_distribution_archives':'retained-at-original-immutable-paths; no duplicate/retirement','SDK_product_SQLite_target_port_executions':0}
(BASE/'premerge-preservation.json').write_text(json.dumps(result,indent=2)+'\n');print('physical',len(physical),'Git',len(gitrows),'archive',archive.stat().st_size)
