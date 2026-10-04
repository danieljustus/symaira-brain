from pathlib import Path
import json,hashlib,subprocess,gzip,tarfile
ROOT=Path('/workspace/symaira-guard770-doctor-boundaries');sha=lambda data:hashlib.sha256(data).hexdigest();r=json.loads(Path('/tmp/symaira-guard770-doctor-final-handoff.json').read_text());p=json.loads(Path('/tmp/symaira-guard9603-root-process.json').read_text())
source={**r['candidate_source_sha256'],**p['candidate_source_sha256'],**p['candidate_manifest_sha256']}
for name,digest in source.items():
 data=(ROOT/name).read_bytes();assert sha(data)==digest,name;assert sha(subprocess.check_output(['git','show',r['validated_source']+':'+name],cwd=ROOT))==digest,name
with subprocess.Popen(['git','archive',p['oracle_ref']],cwd=ROOT,stdout=subprocess.PIPE) as child:
 count=0
 with tarfile.open(fileobj=child.stdout,mode='r|') as tar:
  for member in tar:
   if member.isfile() and member.name in p['go_source_sha256']:
    assert sha(tar.extractfile(member).read())==p['go_source_sha256'][member.name],member.name;count+=1
 assert child.wait()==0;assert count==len(p['go_source_sha256'])
for name,digest in r['proof_sha256'].items():assert sha(Path(name).read_bytes())==digest,name
for name,metadata in r['binaries'].items():assert sha(Path(name).read_bytes())==metadata['sha256'],name
archive=json.loads((ROOT/'migration/evidence/guard-doctor-boundaries-770/baseline-8e3/binary-archive-receipt.json').read_text());assert sha(Path(archive['archive']).read_bytes())==archive['archive_sha256'];oldcount=0
with tarfile.open(archive['archive'],'r:gz') as tar:
 for member in tar:
  if member.isfile():
   key=member.name.removeprefix('./');assert key in archive['ELFs'],key;assert sha(tar.extractfile(member).read())==archive['ELFs'][key]['sha256'];oldcount+=1
assert oldcount==len(archive['ELFs'])==65
out={'head':r['final_head'],'source':r['validated_source'],'source_input_hashes_verified':len(source),'frozenGo_hashes_verified':count,'author_proof_files_verified':len(r['proof_sha256']),'actual_current_binaries_verified':len(r['binaries']),'archived_baseline_ELFs_roundtrip_verified':oldcount,'clean':not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT).strip()};Path('/tmp/symaira-guard9603-root-verification.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out))
