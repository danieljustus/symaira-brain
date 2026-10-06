import hashlib,json,pathlib,re,subprocess
repo=pathlib.Path('/workspace/symaira-usage768-local-files')
prefix='/tmp/symaira-usage768-abf-root-doctor'
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def load(p):return json.loads(pathlib.Path(p).read_text())
def git(*args):return subprocess.check_output(['git','-C',str(repo),*args],text=True).strip()
assert git('rev-parse','HEAD')=='abf20713bacdab562644256e27616a9dd7acb81e'
assert not git('status','--porcelain')
original=repo/'migration/evidence/usage-copilot-kimi-768/original-root-review-7e0b'
retention=load(original/'retention-manifest.json')
assert len(retention['records'])==73
for row in retention['records']:
 assert sha(original/row['retained'])==row['sha256']
 assert sha(row['original'])==row['sha256']
archive=load(original/'archive-receipt.json')
for row in archive['records']:
 assert sha(row['archive'])==row['sha256']
 assert pathlib.Path(row['archive']).stat().st_size==row['bytes']
parentarchive=load('/workspace/oracles/symaira-usage768-files-f241-binaries/archive-receipt.json')
unique={row['sha256']:row for row in parentarchive['executables']}
assert len(parentarchive['executables'])==112 and len(unique)==74
for row in unique.values():assert sha(row['archived_path'])==row['sha256'] and pathlib.Path(row['archived_path']).stat().st_size==row['bytes']
authorroot=repo/'migration/evidence/usage-copilot-kimi-768/linux-owner-00bcc6f'
author=load(authorroot/'verification.json')
for name,digest in author['retention_sha256'].items():assert sha(authorroot/name)==digest
for name,digest in author['binary_sha256'].items():assert sha(name)==digest
reports={}
sources=[]
for label in ['local','files','hermes','reference']:
 p=prefix+'-'+label+'.json';v=load(p)
 assert v['candidate_head']==git('rev-parse','HEAD') and not v['candidate_dirty'] and v['failed']==0
 for name,digest in v['source_sha256'].items():assert sha(repo/name)==digest
 sources.append(v['source_sha256'])
 assert v['cli']['cases']==v['cli']['passed'] and v['cli']['failed']==0 and len(v['negative_controls'])==5
 reports[label]=dict(path=p,sha256=sha(p),cases=v.get('cases',v.get('constructor_request_cases')),full_reports=v.get('full_report_cases',v.get('cases',v.get('constructor_request_cases'))),cli=v['cli']['cases'],controls=5,source_entries=len(v['source_sha256']),binary_sha256=v['cli']['binary_sha256'])
common=set.intersection(*(set(v)for v in sources))
assert len(common)==48
for name in common:assert len({v[name]for v in sources})==1
assert len({v['binary_sha256']['rust']for v in reports.values()})==1
local=load(prefix+'-local.json');accounting=local['original_97_accounting']
assert accounting['original_inputs']==97 and accounting['retained_inputs']==66 and accounting['remaining_route_only']==31
assert len(accounting['cases'])==len({x['id']for x in accounting['cases']})==97
assert local['owner_selection']['native']['passed']==16 and local['owner_selection']['cli']['passed']==16 and local['owner_selection']['negative_controls'][0]['exit']==101
assert local['native_path_semantics']['native']['passed']==22
log=pathlib.Path(prefix+'-tests.log').read_text()
summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
counts=tuple(sum(int(v[i])for v in summaries)for i in range(3));assert counts==(355,0,4)
binaries={}
for path in re.findall(r'Running [^\n]+ \(([^)]+)\)',log):
 p=repo/path;assert p.is_file();binaries[str(p)]=sha(p)
for p in [repo/'target/debug/symbrain',repo/'target/debug/deps/libsymbrain_usage-bc5b4536881a808f.rlib',pathlib.Path(prefix+'-public-probe')]:binaries[str(p)]=sha(p)
extra=load(prefix+'-extra.json');old=load(prefix+'-original-owner-replay.json');cli=load(prefix+'-cli-extra.json');paths=load(prefix+'-path/receipt.json')
assert (extra['cases'],extra['passed'],extra['failed'])==(32,32,0)
assert (old['cases'],old['passed'],old['failed'])==(12,12,0)
assert (cli['cases'],cli['passed'],cli['failed'])==(22,10,12) and all(x['parent_matches']for x in cli['rows'])
assert cli['binary_sha256']['/tmp/symaira-usage768-f759-root-parent-cli']=='9aed2a28060fe84f945db937def4ed10c83f13b15a008d4720ebebf3cd853f38'
assert (paths['cases'],paths['passed'],paths['failed'])==(6065,6013,52)
assert not git('diff','fdfea204..HEAD','--','internal/usage','go.mod','go.sum','cmd/symbrain/cmd_usage.go')
artifacts=sorted(p for p in pathlib.Path('/tmp').glob('symaira-usage768-abf-root-doctor*') if p.is_file() and p.name not in ['symaira-usage768-abf-root-doctor-review-receipt.json','symaira-usage768-abf-root-doctor-review-validation.log'])
artifacts.extend(p for p in pathlib.Path(prefix+'-path').rglob('*')if p.is_file())
for label in reports:artifacts.extend(p for p in pathlib.Path(prefix+'-'+label+'.evidence').rglob('*')if p.is_file())
receipt=dict(disposition='REQUEST_CHANGES',findings=1,priority='P2',group='newly native literal credential sources reach incompatible raw argv/flag syntax diagnostics',head=git('rev-parse','HEAD'),source='00bcc6f51a2fbc00581ca7f6b7af0e868d168971',clean=True,normal_main='e3dbda6cbb95429237d15a9b176209b7d107792c',reports=reports,manifest_counts=[len(v)for v in sources],common_source_entries=len(common),tests=dict(passed=355,failed=0,ignored=4,summaries=len(summaries)),binary_sha256=binaries,original97=dict(cases=97,retained=66,routing_only=31,input_sha256=accounting['input_sha256']),owner_cases=dict(full=16,cli=16,actual_unix_paths=22,wrong_owner_control_exit=101,original_closed=12,supplemental_public=32),new_cli_observations=dict(cases=22,mismatches=12,matching_controls=10,all_archived_parent_match_go=True,receipt=prefix+'-cli-extra.json'),host_windows_algorithm_observations=dict(cases=6065,matches=6013,differences=52,invalid_root_questionmark_or_nonASCII_colon_forms=True,valid_reachable_credential_owner_discrepancy_established=False,not_native_Windows_proof=True,receipt=prefix+'-path/receipt.json'),original_verified=dict(review_artifacts=73,archive_entries=len(archive['records']),author_artifacts=len(author['retention_sha256']),author_binaries=len(author['binary_sha256']),parent_executable_paths=112,parent_unique_digests=74),tracked_go_unchanged=True,strict='all-target/all-feature Clippy; workspace/included-fragment fmt; actionlint; git diff-check passed',native_platforms='actual exact-candidate macOS/Windows CI still required; full768 remains open',commands=dict(environment='subreaper; set-euo; umask022; source /home/agent/.cargo/env; PATH Go1.26.7; CARGO_INCREMENTAL=0 DEV_DEBUG=0 TEST_DEBUG=0; exclusive existing CARGO_TARGET_DIR',ordinary='cargo test --locked -p symbrain-cli -p symbrain-usage --all-targets --all-features',lint='cargo clippy --locked -p symbrain-cli -p symbrain-usage --all-targets --all-features -- -D warnings',gates=['bash scripts/'+s+'/run.sh '+prefix+'-'+n+'.json'for s,n in [('usage-copilot-kimi-oracle','local'),('usage-provider-files-oracle','files'),('usage-hermes-oracle','hermes'),('usage-credential-oracle','reference')]]),artifact_sha256={str(p):sha(p)for p in artifacts},review_report_sha256=sha(prefix+'-review.md'),review_writes_only_tmp=True,target_released=True)
pathlib.Path(prefix+'-review-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('receipt',sha(prefix+'-review-receipt.json'),'actual test/CLI/rlib files',len(binaries),'retained review artifacts',len(artifacts))
