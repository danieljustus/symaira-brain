from pathlib import Path
import gzip,hashlib,io,json,subprocess,tarfile
root=Path('/workspace/symaira-source806-job-doc-markdown');out=root/'migration/evidence/setup-source-windows-job/doc-markdown-ci';out.mkdir(parents=True,exist_ok=True);parent='d0b58ab03f77e847cd92862f0332f5ebc4952b44';sha=lambda b:hashlib.sha256(b).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=root)
assert git('rev-parse','HEAD').decode().strip()==parent
source='rust/symbrain-cli/src/setup_source_windows_job.rs';rawsource=(root/source).read_bytes();assert rawsource==git('show',parent+':'+source)
files=[(root/source,None),(Path('/workspace/review-proof/root-ci-1438-job-111449738122.log'),'d1fa91666261745bbdc161bcfe41e46554871157149a1d698950cd25f766582e'),(Path('/workspace/review-proof/root-ci-1438-job-111449738205.log'),'d9aa935a5ad8adfea2196daa715e6612cb88e002e98a55023788d7e04772ba8e')];records=[];phases=[]
with tarfile.open(out/'complete-original-source-and-native-logs.tar.gz','w:gz') as tar:
 for p,expected in files:
  data=p.read_bytes();st=p.stat();name=p.name;assert expected is None or sha(data)==expected
  info=tarfile.TarInfo(name);info.size=len(data);info.mode=st.st_mode&0o777;info.mtime=st.st_mtime;tar.addfile(info,io.BytesIO(data));records.append(dict(original=str(p),member=name,bytes=len(data),sha256=sha(data),native_metadata={k:getattr(st,k) for k in ['st_mode','st_uid','st_gid','st_size','st_mtime_ns','st_ctime_ns','st_ino','st_dev']}))
  if expected:
   lines=data.splitlines();indices=[i for i,line in enumerate(lines) if b'error: item in documentation is missing backticks' in line]
   assert indices
   for i in indices:
    excerpt=b'\n'.join(lines[max(0,i-2):i+20]);assert b'JobObject' in excerpt and b'doc_markdown' in excerpt and b'`JobObject`' in excerpt
    phases.append({'log':str(p),'first_doc_lint_line_1based':i+1,'excerpt_b64':__import__('base64').b64encode(excerpt).decode(),'clippy_error_matches_original_source_line':True})
with tarfile.open(out/'complete-original-source-and-native-logs.tar.gz') as tar:
 for row in records:assert sha(tar.extractfile(row['member']).read())==row['sha256']
map=[]
for entry in git('ls-tree','-rz',parent).split(b'\0'):
 if not entry:continue
 metadata,path=entry.split(b'\t');mode,kind,oid=metadata.decode().split()
 if kind=='blob':map.append(dict(path=path.decode(),mode=mode,blob=oid))
(out/'whole-original-parent-modes-blobs.json.gz').write_bytes(gzip.compress(json.dumps(dict(parent=parent,whole_tree=map)).encode(),mtime=0))
(out/'retention.json').write_text(json.dumps(dict(parent=parent,records=records,whole_parent_blob_count=len(map),archive_sha256=sha((out/'complete-original-source-and-native-logs.tar.gz').read_bytes()),full_raw_roundtrips=True,actual_failed_native_phases=phases,local_Cargo_SDK_compiler_product_target_port=0),indent=2)+'\n');print('complete retainedlogs/source',len(records),'parentblobs',len(map))
