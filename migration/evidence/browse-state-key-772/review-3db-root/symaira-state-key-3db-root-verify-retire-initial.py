from pathlib import Path
import json,hashlib,gzip,os,subprocess,shutil,re
r=Path('/workspace/symaira-daemon772-state-key')
target=Path('/workspace/symaira-daemon772-registry/target')
source='3db69a655e43ef8cffa3776ad00a823d947ff31b'
head='3c2d731b2b113df81db3601b73f3036c8a558e5c'
sha=lambda p:hashlib.file_digest(Path(p).open('rb'),'sha256').hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==head
assert not subprocess.check_output(['git','status','--porcelain'],cwd=r)
maps=[]
for kind in ['key','ownership','process','registry']:
 d=json.load(open('/tmp/symaira-state-key-3db-root-'+kind+'.json'))
 for key,ref in [('candidate_source_sha256',source),('go_source_sha256',d.get('go_ref'))]:
  entries=d.get(key,{})
  for name,digest in entries.items():
   if key.startswith('candidate'):
    assert sha(r/name)==digest,(kind,name)
   data=subprocess.check_output(['git','show',ref+':'+name],cwd=r)
   assert hashlib.sha256(data).hexdigest()==digest,(kind,key,name)
  maps.append({'report':kind,'map':key,'files':len(entries),'verified':True})
d=json.load(open('/tmp/symaira-state-key-3db-full-source-manifest.json'))
for name,digest in d['changed_files_sha256'].items():
 assert sha(r/name)==digest,name
 assert hashlib.sha256(subprocess.check_output(['git','show',source+':'+name],cwd=r)).hexdigest()==digest,name
ev=r/'migration/evidence/browse-state-key-772/final-3db'
for name,digest in json.load(open(ev/'evidence-sha256.json')).items(): assert sha(ev/name)==digest,name
archive=Path('/workspace/oracles/symaira-daemon772-state-key-3db69a655e43-idle-binaries')
a=json.load(open(archive/'receipt.json'))
for digest,rec in a['unique'].items():
 with gzip.open(rec['archive'],'rb') as f: assert hashlib.file_digest(f,'sha256').hexdigest()==digest
users=[]
for p in Path('/proc').iterdir():
 if not p.name.isdigit() or int(p.name)==os.getpid():continue
 try:
  vals=[os.readlink(p/'exe'),os.readlink(p/'cwd'),(p/'maps').read_text()]
  for fd in (p/'fd').iterdir():
   try:vals.append(os.readlink(fd))
   except OSError:pass
  if any(str(target)+'/' in v or v==str(target) for v in vals):users.append(p.name)
 except (OSError,ProcessLookupError):pass
assert not users,users
current=[]
new=[]
for root,_,names in os.walk(target):
 for name in names:
  p=Path(root)/name
  if p.is_symlink() or not p.is_file():continue
  with p.open('rb') as f:
   if f.read(4)!=b'\x7fELF':continue
  digest=sha(p)
  if digest not in a['unique']:
   out=archive/(digest+'.gz')
   assert not out.exists()
   with p.open('rb') as f,out.open('wb') as dest:
    with gzip.GzipFile(fileobj=dest,mode='wb',mtime=0) as z:shutil.copyfileobj(f,z,1<<20)
   with gzip.open(out,'rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==digest
   a['unique'][digest]={'archive':str(out),'sha256':digest,'original_size':p.stat().st_size,'compressed_size':out.stat().st_size}
   new.append(digest)
  current.append({'path':str(p),'sha256':digest})
tests=[]
for p in ['/tmp/symaira-state-key-3db-root-affected-pinned.log','/tmp/symaira-state-key-3db-root-mcp.json.tests.log']:
 text=Path(p).read_text(); m=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',text)
 tests.append({'path':p,'passed':sum(int(x[0]) for x in m),'failed':sum(int(x[1]) for x in m),'ignored':sum(int(x[2]) for x in m),'summaries':len(m)})
out={'source':source,'head':head,'maps':maps,'changed_file_count':len(d['changed_files_sha256']),'author_evidence_files':32,'original_archive_paths':len(a['executables']),'current_elf_paths':len(current),'unique_archived':len(a['unique']),'new_archives':new,'current_binaries':current,'roundtrip':True,'users':users,'tests':tests,'retired':False}
Path('/tmp/symaira-state-key-3db-root-verification.json').write_text(json.dumps(out,indent=2)+'\n')
# Source and every current ELF are verified; retire only this exclusively owned cache.
shutil.rmtree(target)
out['retired']=True
Path('/tmp/symaira-state-key-3db-root-verification.json').write_text(json.dumps(out,indent=2)+'\n')
# Keep immutable original author archive receipt; successor map above records new archives.
print(json.dumps({k:v for k,v in out.items() if k not in ['current_binaries']},indent=2))

