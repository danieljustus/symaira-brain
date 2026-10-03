from pathlib import Path
import json,hashlib,subprocess,tomllib,tarfile
root=Path('/workspace/symaira-fetch773-proxy-uri');base=root/'browse/port/evidence/fetch-control-773/proxy-uri-e2'; source='e2ecf4026806dd4d9fd219f15f931848b1fd56d9';head='335073aab4bc49b6a4497b38115ed5d8a688374a';dc='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
def sha(data):return hashlib.sha256(data).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=root)
h=json.loads(Path('/tmp/symaira-fetch773-proxy-uri-final-handoff.json').read_bytes());v=json.loads((base/'source-verification.json').read_bytes())
counts={}
for label,manifest,revision,current in [('fetch',v['fetch_source_sha256'],source,True),('harness',v['harness_workflow_sha256'],source,True),('rust',v['rust_build']['source_sha256'],source,True),('go',v['go_build']['source_sha256'],dc,True)]:
 for name,digest in manifest.items():
  assert sha(git('show',revision+':'+name))==digest,(label,name,'git')
  if current:assert sha((root/name).read_bytes())==digest,(label,name,'working')
 counts[label]=len(manifest)
for name,record in h['binaries'].items():assert sha(Path(record['path']).read_bytes())==record['sha256'],name
preserved=json.loads((base/'preservation.json').read_bytes())
for m in preserved['mapping']:
 assert sha((root/m['retained_path']).read_bytes())==m['sha256'],m['retained_path']
 if Path(m['original_path']).is_file():assert sha(Path(m['original_path']).read_bytes())==m['sha256'],m['original_path']
old=json.loads((base/'original522-executables-receipt.json').read_bytes());archive=Path(old['archive']);assert sha(archive.read_bytes())==old['archive_sha256']
verified=0
with tarfile.open(archive,'r:gz') as t:
 for entry in old['executables']:
  member=t.getmember(entry['path']);data=t.extractfile(member).read();assert len(data)==entry['bytes'] and sha(data)==entry['sha256'],entry['path'];verified+=1
  if entry['path']=='debug/examples/control_probe':
   p=Path('/tmp/symaira-fetch773-522-parent-control-probe');p.write_bytes(data);p.chmod(0o755)
oldlock=tomllib.loads(git('show','52236234:browse/Cargo.lock').decode());newlock=tomllib.loads((root/'browse/Cargo.lock').read_text())
identity=lambda d: sorted((p['name'],p['version'],p.get('source',''),p.get('checksum','')) for p in d['package'])
assert identity(oldlock)==identity(newlock)
changes=git('diff','--name-only',source,head).decode().splitlines();assert all(x.startswith(('browse/port/evidence/','browse/docs/')) for x in changes),changes
assert not git('status','--porcelain');assert git('rev-parse','HEAD').decode().strip()==head
assert not git('diff','--name-only','52236234',head,'--','browse/internal','browse/cmd','browse/go.mod','browse/go.sum','browse/port/fixtures'), 'frozen Go/fixture changed'
git('merge-base','--is-ancestor','31de72294521701fc4b1a0bce39f03cc34d72e7e',head)
for path,info in h['retained_evidence'].items():
 digest=info.get('sha256') if isinstance(info,dict) else info
 assert sha((root/path).read_bytes())==digest,(path,info)
result=dict(head=head,source=source,counts=counts,preserved_originals=len(preserved['mapping']),archived_executables=verified,binaries_verified=len(h['binaries']),retained_evidence=len(h['retained_evidence']),evidence_docs_only_changes=len(changes),locked_packages_unchanged=True,frozen_go_fixtures_unchanged=True,clean=True)
Path('/tmp/symaira-fetch773-proxy-uri-independent-provenance.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
