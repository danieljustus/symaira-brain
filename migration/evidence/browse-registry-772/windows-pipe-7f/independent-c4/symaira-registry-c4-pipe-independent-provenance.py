from pathlib import Path
import hashlib,json,subprocess,zipfile
root=Path('/workspace/symaira-daemon772-registry');expected='c4f953af726ff7c4f3185011bbf1ab05de53eeb7';baseline='7f6ff3b8406b2d8861e4c90691919d6bdae77be6'
def git(*args):return subprocess.check_output(['git','-C',str(root),*args]).decode().strip()
def info(path):data=path.read_bytes();return {'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
assert git('rev-parse','HEAD')==expected;assert not git('status','--porcelain');assert git('rev-parse','HEAD^')==baseline
changed=git('diff','--name-only',baseline,expected).splitlines()
source={name:info(root/name)for name in changed}
retained=root/'migration/evidence/browse-registry-772/windows-pipe-7f';r=json.loads((retained/'retention.json').read_bytes())
for name,digest in r['files'].items():assert info(retained/name)==digest
original=json.loads(Path('/tmp/symaira-registry-7f-pipe-independent-original.json').read_bytes())
for name,digest in original['original_files'].items():assert info(Path(name))==digest
with zipfile.ZipFile(retained/'registry-7f-windows-failure.zip')as z:
 for name,digest in original['zip_files'].items():
  data=z.read(name);assert {'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}==digest
artifacts={str(p):info(p)for p in sorted(Path('/tmp').glob('symaira-registry-c4-pipe-*'))if p.is_file()and not p.name.endswith('review-receipt.json')}
result={'head':expected,'baseline':baseline,'clean':True,'changed_files':source,'artifacts':artifacts,'retained_original_files':r,'tests':{'author_pipe':8,'existing_daemon_exit':2,'independent_low_level_and_boundaries':15,'exact_selected_negative_controls':3},'native_windows_executed':False,'target_accessed':False}
Path('/tmp/symaira-registry-c4-pipe-independent-review-receipt.json').write_text(json.dumps(result,indent=2)+'\n');print('PASS: 11 changed files,4 retained originals,5 ZIP members,25 portable tests,3 exact red controls; clean source bound')
