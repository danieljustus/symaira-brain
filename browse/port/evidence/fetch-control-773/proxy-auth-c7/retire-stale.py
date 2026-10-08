from pathlib import Path
import hashlib,json,os,subprocess,tarfile,datetime
plan_path=Path('/tmp/symaira-fetch773-proxy-auth-stale-executable-plan.json');plan=json.loads(plan_path.read_text());target=Path('/workspace/symaira-fetch773-proxy-auth/target');repo=target.parent
assert target.resolve()==target and target.is_dir() and not target.is_symlink()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()=='c7beb3a7a39dbc10f7e84769fccea5cf20e7f09e'
assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
users=[]
for process in Path('/proc').iterdir():
 if not process.name.isdigit():continue
 for link in [process/'exe',process/'cwd',*list((process/'fd').glob('*'))]:
  try:value=os.readlink(link)
  except OSError:continue
  if value==str(target) or value.startswith(str(target)+'/'):users.append({'pid':process.name,'link':str(link),'value':value})
assert not users,users
old=json.loads(Path('/workspace/oracles/symaira-fetch773-335-binaries/receipt.json').read_text());archive=Path(old['archive']);sha=lambda b:hashlib.sha256(b).hexdigest();assert sha(archive.read_bytes())==old['archive_sha256']
retire=[];preserve=[]
for record in plan['candidates']:
 p=Path(record['path'])
 if p.suffix or p.name.startswith('lib'):
  preserve.append(record);continue
 assert p.parent==target/'debug/deps' and p.resolve()==p and not p.is_symlink() and str(p) not in plan['protected']
 assert p.is_file() and p.stat().st_size==record['bytes'] and sha(p.read_bytes())==record['sha256']
 retire.append(record)
assert len(retire)==48 and len(preserve)==16
with tarfile.open(archive,'r:gz') as bundle:
 for record in retire:
  data=bundle.extractfile(record['archive_member']).read();assert len(data)==record['bytes'] and sha(data)==record['sha256']
# Every file and archive were validated before mutation; never touch a dependency or protected/current executable.
for record in retire:
 p=Path(record['path']);assert sha(p.read_bytes())==record['sha256'];p.unlink()
assert all(not Path(x['path']).exists() for x in retire)
assert all(Path(x['path']).is_file() for x in preserve)
receipt=dict(utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),source='c7beb3a7a39dbc10f7e84769fccea5cf20e7f09e',source_clean=True,target_users=users,archive=str(archive),archive_sha256=old['archive_sha256'],original_plan_sha256=sha(plan_path.read_bytes()),retired=retire,preserved_dependencies=preserve,classification_correction='Original64-path plan included16 proc-macro shared libraries because ELF scanning was broader than actual executable-run logs; these were explicitly excluded and remain untouched.48 extensionless archived stale test/CLI paths alone retired.',retired_nominal_bytes=sum(x['bytes'] for x in retire),protected=plan['protected'])
Path('/tmp/symaira-fetch773-proxy-auth-stale-executable-retire-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(len(retire),receipt['retired_nominal_bytes'])
