from pathlib import Path
import json,hashlib
prefix='/tmp/symaira-fetch773-proxy-auth-independent-'
receipt=Path(prefix+'review-receipt.json');original=receipt.read_bytes();digest=hashlib.sha256(original).hexdigest()
assert digest=='7a4ce80e77773ea1988224dc8a53d08dd684ba1fb442e49318a7257188ba4103'
backup=Path(prefix+'review-receipt-original.json')
if backup.exists():assert backup.read_bytes()==original
else:backup.write_bytes(original)
j=json.loads(original);verified=[];changes=[]
assert len(j['evidence'])==44
for name,old in j['evidence'].items():
 p=Path(name);b=p.read_bytes();current=dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
 verified.append(dict(path=name,**current,original_receipt=old,unchanged=current==old))
 if current!=old:changes.append(dict(path=name,original=old,final=current))
assert len(changes)==1 and changes[0]['path']==prefix+'receipt.log'
assert changes[0]['original']==dict(bytes=0,sha256='e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')
assert changes[0]['final']['sha256']=='0732be2eccbe268904223cf84b866d4ab656695760b7c659c114267a3050fb2f'
result=dict(head=j['head'],source=j['source'],original_receipt=dict(path=str(receipt),sha256=digest,bytes=len(original)),immutable_backup=dict(path=str(backup),sha256=digest,bytes=len(original)),verified_files=44,unchanged_files=43,changes=changes,final_manifest=verified,cause='The receipt generator included its own redirected receipt.log while still empty; its final print was written after the manifest snapshot. This supplemental correction records the completed log bytes without rewriting the original receipt.',scope='Evidence retention only. All43 other files including actual raw outputs, review, source proofs and measurement data remain byte-identical. No new tests/builds/target executions/performance measurement. The original review disposition and source remain unchanged.')
out=Path(prefix+'retention-correction.json');out.write_text(json.dumps(result,indent=2)+'\n')
assert receipt.read_bytes()==original
print(json.dumps(dict(original_receipt_sha256=digest,backup_sha256=hashlib.sha256(backup.read_bytes()).hexdigest(),correction_path=str(out),correction_sha256=hashlib.sha256(out.read_bytes()).hexdigest(),verified=44,unchanged=43,completed_log=changes[0]['final']),indent=2))
