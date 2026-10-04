from pathlib import Path
import datetime,gzip,hashlib,json,shutil,subprocess
root=Path('/workspace/symaira-memory649-integrated');target=root/'target';source='1fc5910ebc8ae04b772200ac7cb69afda8fd8716';report=Path('/tmp/symaira-memory649-runtime-1fc5910');archive=Path('/workspace/oracles/symaira-memory649-1fc5910-binaries')
def sha(raw):return hashlib.sha256(raw).hexdigest()
def digest(p):return sha(p.read_bytes())
def users():
 found=[]
 for proc in Path('/proc').iterdir():
  if not proc.name.isdigit():continue
  for link in [proc/'exe',proc/'cwd',*list((proc/'fd').glob('*'))]:
   try:value=str(link.resolve(strict=True))
   except (OSError,RuntimeError):continue
   if value==str(target) or value.startswith(str(target)+'/'):found.append(dict(pid=proc.name,link=str(link),value=value))
 return found
assert not users();assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==source;assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
validation=json.loads((report/'validation.json').read_text());assert validation.get('finished');assert all(s['exit']==0 for s in validation['stages'])
archive.mkdir();payloads={};elfs=[]
oldfolders=[Path('/workspace/oracles/symaira-memory649-'+name+'-failure-binaries') for name in ('25292fe','ab561d5','687289d')]
for path in sorted(target.rglob('*')):
 if not path.is_file():continue
 with path.open('rb') as stream:
  if stream.read(4)!=b'\x7fELF':continue
 raw=path.read_bytes();h=sha(raw)
 if h not in payloads:
  gz=next((folder/(h+'.gz') for folder in oldfolders if (folder/(h+'.gz')).is_file()),archive/(h+'.gz'))
  if not gz.exists():gz.write_bytes(gzip.compress(raw,compresslevel=9,mtime=0))
  compressed=gz.read_bytes();assert gzip.decompress(compressed)==raw
  payloads[h]=dict(gzip=str(gz),gzip_sha256=sha(compressed),bytes=len(raw))
 elfs.append(dict(path=str(path),sha256=h,archive=payloads[h]))
roles=[]
for role in validation['tests']['actual_executed_binaries']:
 assert digest(Path(role['path']))==role['sha256'];assert role['sha256'] in payloads
 roles.append(dict(**role,archive=payloads[role['sha256']]))
cli=validation['actual_cli'];assert digest(Path(cli['path']))==cli['sha256'];assert cli['sha256'] in payloads
original_receipts=[];checked=set()
for folder in oldfolders:
 p=folder/'receipt.json';d=json.loads(p.read_text())
 for item in d['ELF_files']:
  a=item['archive'];gz=Path(a['gzip'])
  if str(gz) not in checked:
   compressed=gz.read_bytes();assert sha(compressed)==a['gzip_sha256'];raw=gzip.decompress(compressed);assert len(raw)==a['bytes'] and sha(raw)==item['sha256'];checked.add(str(gz))
 original_receipts.append(dict(path=str(p),sha256=digest(p),files=len(d['ELF_files']),unique=d['unique_payloads'],source=d['source'],tests=d['tests']))
binary_receipt=dict(source=source,tests=validation['tests'],actual_executed_roles=roles,cli=dict(**cli,archive=payloads[cli['sha256']]),ELF_files=elfs,unique_payloads=len(payloads),all_archives_roundtrip_verified=True,previous_failures=original_receipts,previous_unique_archives_fresh_verified=len(checked),unknown_compiler_roles_not_claimed_executed=True,timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),target_users=users());assert not binary_receipt['target_users'];(archive/'receipt.json').write_text(json.dumps(binary_receipt,indent=2)+'\n')
evidence=root/'migration/evidence/memory-historical-649/final-1fc5910';evidence.mkdir();retained=evidence/'retained';retained.mkdir();files={}
for p in sorted(report.rglob('*')):
 if not p.is_file():continue
 raw=p.read_bytes();h=sha(raw);gz=retained/(h+'.gz')
 if not gz.exists():gz.write_bytes(gzip.compress(raw,compresslevel=9,mtime=0))
 assert gzip.decompress(gz.read_bytes())==raw
 files[str(p.relative_to(report))]=dict(original=str(p),sha256=h,bytes=len(raw),retained=str(gz.relative_to(evidence)),gzip_sha256=digest(gz))
for name in ('validation.json','all-target-tests.log','strict-clippy.log','fmt.log','actionlint.log','diffcheck.log','build-actual-cli.log'):
 shutil.copyfile(report/name,evidence/name)
shutil.copyfile(archive/'receipt.json',evidence/'binaries.json');shutil.copyfile(Path(__file__),evidence/'preserve.py')
prep=Path('/tmp/symaira-memory649-runtime-25292fe-prep');shutil.copyfile(prep/'run-1fc5910.sh',evidence/'run.sh');shutil.copyfile(prep/'source.json',evidence/'original-frozen-source.json')
prior={name:digest(root/name) for name in subprocess.check_output(['git','ls-files','migration/evidence/memory-historical-649'],cwd=root,text=True).splitlines()}
receipt=dict(source=source,validation_sha256=digest(evidence/'validation.json'),binary_receipt_sha256=digest(evidence/'binaries.json'),report_files=files,all_fresh_proof_gzip_roundtrips=True,all_prior_tracked_evidence_sha256=prior,original_failures=original_receipts,archive_receipt=str(archive/'receipt.json'),free_bytes=shutil.disk_usage(root).free,target_users=users(),runtime_scope='Author full Linux actual runtime validation only. Different-author full runtime review, native Windows/macOS, protected CI and release remain pending. No 649/758 closure.')
assert not receipt['target_users'];(evidence/'retention.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(dict(source=source,tests={k:v for k,v in validation['tests'].items() if k!='actual_executed_binaries'},CLI=cli,ELF_files=len(elfs),unique=len(payloads),roles=len(roles),report_files=len(files),original_proof_files=len(prior),target_users=0,free_bytes=receipt['free_bytes']),indent=2))
