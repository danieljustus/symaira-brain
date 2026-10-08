from pathlib import Path
import hashlib,json,subprocess,tarfile
repo=Path('/workspace/symaira-fetch773-proxy-auth');base=repo/'browse/port/evidence/fetch-control-773/proxy-auth-c7';source='c7beb3a7a39dbc10f7e84769fccea5cf20e7f09e';head='5b94a66159db34d335bc32b98bbcd2cb1dfbe5d9'
sha=lambda b:hashlib.sha256(b).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head
assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
subprocess.run(['git','merge-base','--is-ancestor','e3dbda6cbb95429237d15a9b176209b7d107792c',source],cwd=repo,check=True)
v=json.loads((base/'validation.json').read_text()); counts={}
for group,manifest in v['verification']['manifests'].items():
 for name,digest in manifest.items():
  revision='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c' if group=='go' else source
  assert sha((repo/name).read_bytes())==sha(subprocess.check_output(['git','show',revision+':'+name],cwd=repo))==digest,(group,name)
 counts[group]=len(manifest)
for record in json.loads((base/'preservation.json').read_text())['mapping']:
 assert sha((repo/record['retained_path']).read_bytes())==sha(Path(record['original_path']).read_bytes())==record['sha256']
for record in [*v['binaries'].values(),*v['elf_artifacts']]:assert sha(Path(record['path']).read_bytes())==record['sha256'],record['path']
archive=json.loads((repo/'browse/port/evidence/fetch-control-773/independent-review-335/binary-preservation.json').read_text())
assert sha(Path(archive['archive']).read_bytes())==archive['archive_sha256']
expected={x['member']:x for x in archive['executables']}
with tarfile.open(archive['archive'],'r:gz') as tar:
 for member in tar:
  if member.name in expected:
   data=tar.extractfile(member).read();record=expected.pop(member.name)
   assert len(data)==record['bytes'] and sha(data)==record['sha256'],member.name
assert not expected,expected
# Every retained previous-review file must equal its immutable final Git object.
original=repo/'browse/port/evidence/fetch-control-773/independent-review-335'
for path in original.rglob('*'):
 if path.is_file():assert path.read_bytes()==subprocess.check_output(['git','show',head+':'+str(path.relative_to(repo))],cwd=repo)
changes=subprocess.check_output(['git','diff','--name-only',source,head],cwd=repo,text=True).splitlines()
assert all(x.startswith('browse/port/evidence/') or x=='browse/docs/rust-port/adr-honest-fetch-control-773.md' for x in changes)
result=dict(head=head,source=source,manifests=counts,current_elf_paths=len(v['elf_artifacts']),primary_binaries=v['binaries'],prior_archive_entries=len(archive['executables']),prior_archive_sha256=archive['archive_sha256'],mapped_proofs=len(json.loads((base/'preservation.json').read_text())['mapping']),clean=True,evidence_only_successor=True)
Path('/tmp/symaira-fetch773-proxy-auth-independent-provenance.json').write_text(json.dumps(result,indent=2)+'\n');print({k:v for k,v in result.items() if k!='primary_binaries'})
