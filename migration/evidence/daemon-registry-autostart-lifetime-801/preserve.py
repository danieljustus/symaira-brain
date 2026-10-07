"""Lossless pre-edit original retention; no product, SDK or native processes."""
from pathlib import Path
import hashlib,io,json,os,subprocess,tarfile,zipfile
O=Path(__file__).parent;R=Path('/workspace/symaira-daemon801-progress-observer');P=Path('/workspace/review-proof/review-pr800-daemon801-progress-correction');H='8a95b40e854724384b4a551f316215662d7b5815';sha=lambda b:hashlib.sha256(b).hexdigest();git=lambda *a:subprocess.check_output(['git',*a],cwd=R)
assert git('rev-parse','HEAD').decode().strip()==H and not git('status','--porcelain')
manifest=json.loads((P/'proof-manifest.json').read_bytes());assert len(manifest)==193
files=[]
for row in manifest:
 p=Path(row['path']);b=p.read_bytes();assert (len(b),sha(b))==(row['bytes'],row['sha256']);files.append((p,'prior/'+str(p.relative_to(P))))
files.append((P/'proof-manifest.json','prior/proof-manifest.json'))
logs=[('111464695388',147887,'c6b550caf822a7e21a6fee51b9914e418bec64ef0b14f55633d622a1cd450ddc'),('111464695398',154599,'cfaae08f1c8a5fc81197118cfe4e562e58c697398dc52c47a494617ae62e8d28')]
for job,length,digest in logs:
 p=Path('/workspace/review-proof')/('root-ci-1535-job-'+job+'.log');b=p.read_bytes();assert len(b)==length and sha(b)==digest;files.append((p,'native/'+p.name));p=p.with_suffix('.content.json');files.append((p,'native/'+p.name))
for rel in ['browse/port/harness/daemon_registry.py','browse/port/harness/registry_cli_process.py','browse/port/harness/registry_progress.py','browse/port/harness/registry_compare.py','browse/port/harness/test_registry_progress.py','browse/port/harness/test_registry_cli_process.py','browse/port/harness/daemon_registry_cli.py','browse/port/harness/run.py','browse/port/harness/daemon_state_key_ownership.py','.github/workflows/browse-daemon-native.yml','AGENTS.md']:
 p=R/rel;expected=git('show',H+':'+rel)
 if not p.is_file():
  p=O/'original-Git-only-source'/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(expected)
 b=p.read_bytes();assert b==expected;files.append((p,'source/'+rel))
records=[];archive=O/'complete-before-edit.tar.gz'
with tarfile.open(archive,'w:gz',format=tarfile.PAX_FORMAT) as t:
 for p,name in files:
  b=p.read_bytes();st=p.stat();t.add(p,arcname=name,recursive=False);records.append(dict(original=str(p),member=name,bytes=len(b),sha256=sha(b),mode=st.st_mode,uid=st.st_uid,gid=st.st_gid,mtime_ns=st.st_mtime_ns,atime_ns=st.st_atime_ns))
with tarfile.open(archive,'r:gz') as t:
 assert len(t.getmembers())==len(records)
 for row in records:
  m=t.getmember(row['member']);b=t.extractfile(m).read();assert (len(b),sha(b),m.mode,m.uid,m.gid)==(row['bytes'],row['sha256'],row['mode']&0o7777,row['uid'],row['gid'])
# Preserve full committed source inventory as recreatable Git bodies/modes.
source=[]
for entry in git('ls-tree','-rz',H).split(b'\0'):
 if not entry:continue
 meta,name=entry.split(b'\t');mode,kind,oid=meta.split();b=git('cat-file','blob',oid.decode());source.append(dict(path=os.fsdecode(name),git_mode=mode.decode(),git_blob=oid.decode(),bytes=len(b),sha256=sha(b)))
(O/'original-whole-source.json').write_text(json.dumps(dict(head=H,files=source),indent=2)+'\n')
# Reuse Root's complete native archive ledger; verify every payload instead of duplicating it.
root=Path('/workspace/review-proof/root-daemon801-8a95-native-originals/receipt.json');d=json.loads(root.read_bytes());assert d['head']==H;zips=[]
for row in d['whole_original_archives']:
 p=Path(row['zip']);b=p.read_bytes();assert (len(b),sha(b))==(row['bytes'],row['sha256']);members=row['members']
 with zipfile.ZipFile(io.BytesIO(b)) as z:
  assert len(z.infolist())==len(members)
  for m in members:
   info=z.getinfo(m['name']);raw=z.read(info);assert (len(raw),sha(raw),info.CRC,info.external_attr,list(info.date_time))==(m['bytes'],m['sha256'],m['CRC'],m['external_attr'],m['date_time'])
   if m.get('extracted'):assert Path(m['extracted']).read_bytes()==raw
 zips.append(dict(path=str(p),bytes=len(b),sha256=sha(b),members=len(members),full_payload_CRC_metadata_verified=True))
assert sum(z['members'] for z in zips)==9762
receipt=dict(original=H,archive=str(archive),archive_bytes=archive.stat().st_size,archive_sha256=sha(archive.read_bytes()),members=records,full_original_prior193_verified=True,whole_source_files=len(source),whole_source_manifest_sha256=sha((O/'original-whole-source.json').read_bytes()),native_original_ledger=str(root),native_original_ledger_sha256=sha(root.read_bytes()),all9762native_payloads=zips,no_current_product_SDK_compiler_Target_ports=True)
(O/'before-edit-retention.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(dict(archive_members=len(records),source_files=len(source),native_members=9762,archive_sha256=receipt['archive_sha256'])))
