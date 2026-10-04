import gzip, hashlib, io, json, os, pathlib, subprocess, tarfile
repo=pathlib.Path('/workspace/symaira-brain765-main-integration')
out=repo/'migration/evidence/brain-config13/main935-integration'
out.mkdir(parents=True,exist_ok=True)
heads=['e30676fdaced483a3cd1588a78253a3f15096d04','9353520a34c5819c7d0a6cd58d3bfaa2b22b045b']
sha=lambda b:hashlib.sha256(b).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=repo)
manifest=[]; maps={}; content={}
for head in heads:
 rows=[]
 for entry in git('ls-tree','-rz',head).split(b'\0'):
  if not entry:continue
  meta,path=entry.split(b'\t',1);mode,kind,oid=meta.decode().split();path=os.fsdecode(path)
  if kind!='blob':continue
  rows.append({'path':path,'mode':mode,'git_blob':oid})
  if path.startswith(('rust/','scripts/','.github/','docs/adr/')) or path in ['Cargo.toml','Cargo.lock','MIGRATION-RUST.md','migration/contract-matrix.csv','AGENTS.md'] or path.startswith('migration/fixtures/brain-config13/'):
   data=git('cat-file','blob',oid);name='parents/'+head+'/'+path
   content[name]=(data,int(mode,8)&0o777);manifest.append({'name':name,'sha256':sha(data),'bytes':len(data),'git_mode':mode,'git_blob':oid})
 maps[head]=rows
for directory in [pathlib.Path('/workspace/review-proof/review-pr800-brain765-startup-e306'),pathlib.Path('/workspace/review-proof/brain765-main-integration/before')]:
 for p in sorted(directory.rglob('*')):
  if not p.is_file():continue
  data=p.read_bytes();st=p.stat();name='original-proofs/'+directory.name+'/'+str(p.relative_to(directory))
  content[name]=(data,st.st_mode&0o777);manifest.append({'name':name,'original':str(p),'sha256':sha(data),'bytes':len(data),'native_metadata':{k:getattr(st,k) for k in ['st_mode','st_uid','st_gid','st_size','st_mtime_ns','st_ctime_ns','st_ino','st_dev']}})
with tarfile.open(out/'before-merge.tar.gz','w:gz',compresslevel=9) as archive:
 for name,(data,mode) in content.items():
  info=tarfile.TarInfo(name);info.size=len(data);info.mode=mode;info.mtime=0;archive.addfile(info,io.BytesIO(data))
with tarfile.open(out/'before-merge.tar.gz','r:gz') as archive:
 assert len(archive.getmembers())==len(content)
 for info in archive.getmembers():assert archive.extractfile(info).read()==content[info.name][0]
raw=json.dumps({'parents':heads,'all_parent_git_modes_blobs':maps,'archived_complete_source_and_original_proofs':manifest},sort_keys=True).encode()
(out/'before-merge-manifest.json.gz').write_bytes(gzip.compress(raw,mtime=0))
(out/'retention.json').write_text(json.dumps({'parents':heads,'source_and_proof_members':len(content),'all_parent_blob_counts':{k:len(v) for k,v in maps.items()},'archive_sha256':sha((out/'before-merge.tar.gz').read_bytes()),'manifest_sha256':sha((out/'before-merge-manifest.json.gz').read_bytes()),'all_members_roundtrip':True,'historical_ELF_archives_reused_verified':'before/full-original-verification.log; no copied ELF payloads','runtime':0},indent=2)+'\n')
print((out/'retention.json').read_text())
