import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

repo=Path('/workspace/symaira-usage768-remaining')
source='c0ae71c0bc0a5e39c65458484f1024d5940a19aa'
frozen='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
target=repo/'target'
out=repo/'migration/evidence/usage-next-768/final-linux'
archive=Path('/workspace/oracles/symaira-usage768-next-c0ae71-binaries')
sha=lambda b:hashlib.sha256(b).hexdigest()
git=lambda *a:subprocess.check_output(['git','-C',str(repo),*a])
assert git('rev-parse','HEAD').decode().strip()==source
assert not git('status','--porcelain')
users=[]
for proc in Path('/proc').iterdir():
 if not proc.name.isdigit():continue
 for link in [proc/'exe',proc/'cwd',*list((proc/'fd').glob('*'))]:
  try:value=os.readlink(link)
  except OSError:continue
  if value==str(target)or value.startswith(str(target)+'/'):users.append(dict(pid=proc.name,link=str(link),value=value))
assert not users,users
reports={name:json.loads(Path(f'/tmp/symaira-usage768-next-final-{name}.json').read_text())for name in ['local','next','files','hermes','refs','fetch']}
union={}
for name,r in reports.items():
 assert r.get('candidate_head',r.get('candidate_source_commit'))==source and r['candidate_dirty']is False
 for path,digest in r.get('source_sha256',{}).items():
  data=(repo/path).read_bytes()
  assert sha(data)==digest and data==git('show',source+':'+path),(name,path)
  assert path not in union or union[path]==digest
  union[path]=digest
common=set.intersection(*(set(reports[name]['source_sha256'])for name in ['local','next','files','hermes','refs']))
for path,digest in reports['next']['frozen_source_sha256'].items():assert sha(git('show',frozen+':'+path))==digest,path
assert reports['next']['cli_build']['candidate_restored_byte_identical']
assert reports['local']['argv_diagnostics']['build']['candidate_restored_byte_identical']
actual_cli=target/'debug/symbrain'
assert sha(actual_cli.read_bytes())==reports['next']['cli_build']['binaries']['rust']['sha256']
rawtests=Path('/tmp/symaira-usage768-next-final-tests.log').read_text()
summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',rawtests)
counts=[sum(int(r[i])for r in summaries)for i in range(3)]
assert counts==[355,0,5],counts
paths=set()
for folder in ['rust/symbrain-cli/src','rust/symbrain-cli/tests','rust/symbrain-usage','scripts/usage-next-oracle','scripts/usage-copilot-kimi-oracle','scripts/usage-provider-files-oracle','scripts/usage-hermes-oracle','scripts/usage-credential-oracle','scripts/usage-fetch-oracle']:
 paths.update(p for p in (repo/folder).rglob('*')if p.is_file()and '__pycache__'not in p.parts)
paths.update(repo/p for p in ['Cargo.toml','Cargo.lock','.github/workflows/ci.yml','docs/adr/usage-next-credential-scope-768.md','docs/adr/usage-cli-argv-bytes-768.md','migration/evidence/usage-remaining-baseline-768/symaira-usage768-remaining-device-input.json'])
scope={}
for path in sorted(paths):
 relative=str(path.relative_to(repo));data=path.read_bytes();assert data==git('show',source+':'+relative);scope[relative]=sha(data)
out.mkdir(parents=True);archive.mkdir(parents=True)
proof=[];binary_paths=[];package_archives=[]
inputs=list(Path('/tmp').glob('symaira-usage768-next-final-*.json'))+list(Path('/tmp').glob('symaira-usage768-next-final-*.log'))
for folder in Path('/tmp').glob('symaira-usage768-next-final-*.evidence'):inputs.extend(p for p in folder.rglob('*')if p.is_file())
for path in sorted(set(inputs)):
 data=path.read_bytes()
 if data.startswith(b'\x7fELF'):binary_paths.append(path);continue
 if path.name.endswith('.tar.gz'):
  dest=archive/'package-archives'/path.parent.name/path.name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,dest)
  assert dest.read_bytes()==data;package_archives.append(dict(original=str(path),path=str(dest),bytes=len(data),sha256=sha(data)));continue
 relative=path.relative_to('/tmp');dest=out/'raw'/Path(str(relative)+'.gz');dest.parent.mkdir(parents=True,exist_ok=True)
 compressed=gzip.compress(data,compresslevel=6,mtime=0);dest.write_bytes(compressed);assert gzip.decompress(dest.read_bytes())==data
 proof.append(dict(original=str(path),tracked=str(dest.relative_to(repo)),bytes=len(data),sha256=sha(data),compressed_sha256=sha(compressed),roundtrip_verified=True))
for path in target.rglob('*'):
 if path.is_file()and not path.is_symlink():
  with path.open('rb')as f:header=f.read(4)
  if header==b'\x7fELF':binary_paths.append(path)
binary_paths.append(actual_cli);records=[];unique={}
for path in sorted(set(binary_paths)):
 data=path.read_bytes();digest=sha(data);dest=archive/'sha256'/(digest+'.gz')
 if digest not in unique:
  dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(gzip.compress(data,compresslevel=1,mtime=0));assert gzip.decompress(dest.read_bytes())==data
  unique[digest]=dict(path=str(dest),bytes=len(data),compressed_bytes=dest.stat().st_size,sha256=digest,compressed_sha256=sha(dest.read_bytes()),roundtrip_verified=True)
 records.append(dict(original=str(path),bytes=len(data),sha256=digest,archive=str(dest)))
binary_receipt=dict(source=source,target=str(target),target_users=users,scope='Every current actual ELF in the exclusive target plus actual copied Go/native/provider-test/parent/sentinel executables. Lossless gzip SHA/length roundtrip verified. Original sources, targets and archives retained; no retirement.',records=records,unique=unique,package_archives=package_archives)
(archive/'receipt.json').write_text(json.dumps(binary_receipt,indent=2)+'\n')
receipt=dict(source=source,normal_main='2b6d49f250a81650eda1b93cec7d859a171873bb',accepted_parent='8803847278546f3161eaaa84d2e65bc2eec594a2',publication_parent='b8621bc26e8d9c5985e8517968d679c127fa152a',frozen_go=frozen,target=str(target),target_users=users,source_manifest_union=union,common_source_sha256={p:union[p]for p in sorted(common)},complete_scope_sha256=scope,frozen_source_sha256=reports['next']['frozen_source_sha256'],source_manifest_totals={name:len(r.get('source_sha256',{}))for name,r in reports.items()},original_gate_results={name:{k:r[k]for k in ['cases','passed','failed','full_report_cases','retained_gates','constructor_request_cases']if k in r and isinstance(r[k],(int,bool,str))}for name,r in reports.items()},ordinary=dict(passed=355,failed=0,ignored=5,parent_summaries=len(summaries),ignored_scope='The four original explicit constructor oracles plus the new complete real TLS/early-argv oracle were all actually run.'),strict=dict(clippy='passed all-targets/all-features CLI+Usage -D warnings',fmt='passed workspace',actionlint='passed ci.yml',diff_check='passed'),actual_cli=dict(path=str(actual_cli),bytes=actual_cli.stat().st_size,sha256=sha(actual_cli.read_bytes())),provider_test_binaries=reports['next']['provider_test_binaries'],copied_cli_binaries=reports['next']['cli_build']['binaries'],binary_archive_receipt=str(archive/'receipt.json'),binary_archive_receipt_sha256=sha((archive/'receipt.json').read_bytes()),proof_manifest=proof,raw_proof_files=len(proof),native_platforms='Author Linux only; exact-head native Linux/macOS/Windows CI and different-author full-layer review still required. No full issue768 closure.',remaining_gates='Distinct Claude/Copilot accounts, unsafe/invalid/control identity sources, unsupported bases/workspace/numeric conversion, Windows home and automatic macOS host Keychain remain bounded.',commands=['bash scripts/usage-copilot-kimi-oracle/run.sh /tmp/symaira-usage768-next-final-local.json','bash scripts/usage-next-oracle/run.sh /tmp/symaira-usage768-next-final-next.json','bash scripts/usage-provider-files-oracle/run.sh /tmp/symaira-usage768-next-final-files.json','bash scripts/usage-hermes-oracle/run.sh /tmp/symaira-usage768-next-final-hermes.json','bash scripts/usage-credential-oracle/run.sh /tmp/symaira-usage768-next-final-refs.json','bash scripts/usage-fetch-oracle/run.sh /tmp/symaira-usage768-next-final-fetch.json','cargo test --locked -p symbrain-cli -p symbrain-usage --all-features','cargo clippy --locked -p symbrain-cli -p symbrain-usage --all-targets --all-features -- -D warnings','cargo fmt --all -- --check','/workspace/toolchains/bin/actionlint .github/workflows/ci.yml','git diff --check'],environment=dict(subreaper='/tmp/symaira-subreaper.py',umask='022',go_path='/workspace/toolchains/go1.26.7/bin',cargo_target_dir=str(target),cargo_incremental='0',cargo_profile_dev_debug='0',cargo_profile_test_debug='0',cargo_build_jobs='2'))
(out/'validation.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(dict(source=source,common_sources=len(common),manifest_totals=receipt['source_manifest_totals'],proof_files=len(proof),ELF_paths=len(records),unique_ELFs=len(unique),ordinary=counts,summaries=len(summaries),actual_cli=receipt['actual_cli']['sha256'])))
