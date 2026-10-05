from pathlib import Path
import json,hashlib,gzip,subprocess
root=Path('/workspace/symaira-daemon772-state-key-discovery-clean'); original=Path('/workspace/symaira-daemon772-state-key-discovery'); oracle=Path('/workspace/oracles/daemon772-go-source')
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
prov=json.loads(Path('/tmp/symaira-state-key-discovery-independent-static-provenance.json').read_text())
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=original,text=True).strip()=='48fc3639e2265756f3bf8f9c4c914afa971ea5da'
assert not subprocess.check_output(['git','status','--porcelain'],cwd=original)
for row in prov['source_maps']:assert digest(original/row['path'])==row['sha256']
for row in prov['frozen_Go_files']:assert digest(oracle/row['path'])==row['sha256']
assert not subprocess.check_output(['git','status','--porcelain'],cwd=oracle)
folder=root/'migration/evidence/browse-state-key-772/discovery-request-48fc';receipt=json.loads((folder/'receipt.json').read_text())
for row in receipt['proofs']:
 data=(folder/row['retained']).read_bytes();raw=gzip.decompress(data)
 assert hashlib.sha256(data).hexdigest()==row['gzip_sha256'] and hashlib.sha256(raw).hexdigest()==row['sha256'] and len(raw)==row['bytes']
current=Path('/tmp/symaira-state-key-discovery-clean-718-static.json');r=json.loads(current.read_text())
r['original48fc_immutable14source_maps_verified']=len(prov['source_maps']);r['original1219_frozen_Go_maps_verified']=len(prov['frozen_Go_files']);r['new7_independent_review_proofs_roundtrip']=len(receipt['proofs'])
r['unported_compatibility_boundaries']=['Go runtime stack-selective bisect identity and marker/stack reporting','Windows bare argv0/raw GetCommandLine identity','Windows Lstat ACL/sharing/retained-handle identity differences']
r['full_discovery_compatibility']=False;r['source_only']=True;r['runtime_runs']=0;r['targets_absent']={str(root/'target'):not(root/'target').exists(),str(root/'browse/target'):not(root/'browse/target').exists(),'/workspace/symaira-daemon772-state-key-osargs/target':not Path('/workspace/symaira-daemon772-state-key-osargs/target').exists()}
current.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'source':r['source_head'],'original14':14,'originalGo1219':1219,'originalRoot57':57,'newIndependent7':7,'new_maps':len(r['candidate_source_sha256']),'runtime':0,'full_parity':False}))
