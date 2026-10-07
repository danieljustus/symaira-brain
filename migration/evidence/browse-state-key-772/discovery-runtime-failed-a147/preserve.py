from pathlib import Path
import json,gzip,hashlib,shutil,subprocess
root=Path('/workspace/symaira-daemon772-discovery-runtime');target=root/'target';prior=json.loads(Path('/tmp/symaira-discovery-runtime-b17-nonport/validation.json').read_text())['failed_source_bound_ELF']['unique'];out=Path('/tmp/symaira-discovery-runtime-a147-before-fixture-fix');out.mkdir();new=out/'archives';new.mkdir()
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  while c:=f.read(1024*1024):h.update(c)
 return h.hexdigest()
rows=[];unique={};added=0
for p in target.rglob('*'):
 if not p.is_file():continue
 with p.open('rb') as f:magic=f.read(4)
 if magic!=b'\x7fELF':continue
 h=sha(p)
 if h not in unique:
  if h in prior:
   info=prior[h];a=Path(info['archive']);assert sha(a)==info['gzip_sha256']
  else:
   a=new/(h+'.gz')
   with p.open('rb') as s,a.open('wb') as d:
    with gzip.GzipFile(fileobj=d,mode='wb',mtime=0) as z:shutil.copyfileobj(s,z)
   info={'archive':str(a),'gzip_sha256':sha(a)};added+=1
  v=hashlib.sha256();length=0
  with gzip.open(a,'rb') as f:
   while c:=f.read(1024*1024):v.update(c);length+=len(c)
  assert v.hexdigest()==h and length==p.stat().st_size
  unique[h]=info
 rows.append({'path':str(p),'sha256':h,'bytes':p.stat().st_size})
source=subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip();assert source=='a147cedb6fd003f257cc491fcb94c45db4291400';assert not subprocess.check_output(['git','-C',str(root),'status','--porcelain'],text=True)
for n in ['browse/crates/symbrowse-core/src/policy_guard.rs','browse/crates/symbrowse-core/tests/policy_guard.rs']:
 d=out/'source'/n;d.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/n,d)
shutil.copyfile('/tmp/symaira-discovery-runtime-a147-policy-guard-diagnostic.log',out/'cargo-diagnostic.log');shutil.copytree('/tmp/symaira-discovery-runtime-a147-policy-guard-reproduction',out/'first-real-failure');shutil.copytree('/tmp/symaira-discovery-runtime-b17-nonport/failed-owned-fixture',out/'original-owned-fixture')
receipt={'source':source,'status':'GENUINE_DESCENDANT_STDIN_RACE_RETAINED_BEFORE_FIX','all_target_ELF_paths':rows,'unique':unique,'new_archive_payloads':added,'roundtrip_verified':True,'actual_role':{'path':str(target/'debug/deps/policy_guard-12661e3eaa035ad6'),'sha256':sha(target/'debug/deps/policy_guard-12661e3eaa035ad6'),'cargo':{'passed':2,'failed':0},'first_diagnostic_child':{'passed':0,'failed':1,'error':'guard decide failed: Broken pipe (os error 32)'}},'stale_transferred_roles_are_not_current_execution':True}
(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({'paths':len(rows),'unique':len(unique),'new_payloads':added,'original_reused':len(unique)-added,'source':source},indent=2))
