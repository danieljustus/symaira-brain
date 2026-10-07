from pathlib import Path
import json,hashlib,subprocess
root=Path('/workspace/symaira-guard770-diagnostics');h=json.loads(Path('/tmp/symaira-guard770-diagnostics-final-handoff.json').read_text());v=h['clean_validation'];head=h['candidate_head'];source=h['validated_source'];parent=h['approved_published_parent'];dc='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
def sha(b):return hashlib.sha256(b).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=root)
counts={}
for label,manifest,ref in [('candidate',v['candidate_runtime_harness_source_sha256'],source),('shared',v['unchanged_shared_runtime_sha256'],parent),('go',v['frozen_go_source_sha256'],dc)]:
 for name,digest in manifest.items():
  assert sha(git('show',ref+':'+name))==digest,(label,name,'immutable')
  assert sha((root/name).read_bytes())==digest,(label,name,'working')
 counts[label]=len(manifest)
for name,digest in v['unchanged_shared_runtime_sha256'].items():assert sha(git('show',head+':'+name))==digest
preserved={}
for p in (root/'migration/evidence/guard-diagnostics-770').rglob('preservation.json'):
 x=json.loads(p.read_text())
 for entry in x['mapping']:
  assert sha((root/entry['retained']).read_bytes())==entry['sha256'],entry['retained']
  original=Path(entry['original'])
  if original.is_absolute() and original.is_file():assert sha(original.read_bytes())==entry['sha256'],entry['original']
 preserved[str(p.relative_to(root))]=len(x['mapping'])
for name,info in h['retained_evidence'].items():assert sha((root/name).read_bytes())==info['sha256'],name
for name,info in h['actual_native_ELFs'].items():assert sha((root/name).read_bytes())==info['sha256'],name
changes=git('diff','--name-only',source,head).decode().splitlines();assert all(x.startswith('migration/evidence/guard-diagnostics-770/clean43/') for x in changes)
assert not git('status','--porcelain');assert git('rev-parse','HEAD').decode().strip()==head
git('merge-base','--is-ancestor',parent,head)
assert not git('diff','--name-only',parent,head,'--','guard','go.mod','go.sum','Cargo.toml','Cargo.lock','rust/symbrain-cli','rust/symbrain-guard-core','rust/symbrain-audit')
p=json.loads(Path('/tmp/symaira-guard770-diagnostics-independent-process.json').read_text());assert p['go_source_sha256']==v['frozen_go_source_sha256'] and p['supplemental_go_entry_sha256']==v['supplemental_go_entry_sha256']
assert sha((root/'target/debug/symguard').read_bytes())==v['binaries_sha256']['rust']
base=json.loads(git('show',parent+':migration/evidence/guard-standalone-770/linux-windows-path-correction.json'));oldids=[x['id'] for x in base['results']];ids=[x['id'] for x in p['results']];assert all((x.replace('doctor-unported-invalid-default','doctor-invalid-default').replace('doctor-unported-invalid-threshold','doctor-invalid-threshold')) in ids for x in oldids)
for oldcase in base['results']:
 name=oldcase['id'].replace('doctor-unported-invalid-default','doctor-invalid-default').replace('doctor-unported-invalid-threshold','doctor-invalid-threshold');newcase=next(x for x in p['results'] if x['id']==name);assert (oldcase['args'],oldcase['state'])==(newcase['args'],newcase['state'])
original=json.loads((root/'migration/evidence/guard-diagnostics-770/original-wip-4a9/symaira-guard770-diagnostics-process.json').read_bytes()) if (root/'migration/evidence/guard-diagnostics-770/original-wip-4a9/symaira-guard770-diagnostics-process.json').is_file() else None
if original:assert len(original['results'])==94 and all(x['id'] in ids for x in original['results'])
result=dict(head=head,source=source,parent=parent,counts=counts,preservation=preserved,retained_evidence=len(h['retained_evidence']),ELFs=len(h['actual_native_ELFs']),evidence_only_successor=len(changes),native_sha256=v['binaries_sha256']['rust'],published80_retained=len(oldids),original94_retained=True if original else 'verified by original replay input later',unchanged_shared_runtime=True,frozen_go_and_fixtures_unchanged=True,clean=True)
Path('/tmp/symaira-guard770-diagnostics-independent-provenance.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
