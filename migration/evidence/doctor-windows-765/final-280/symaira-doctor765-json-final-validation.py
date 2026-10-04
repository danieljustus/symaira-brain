from pathlib import Path
import base64,gzip,hashlib,json,re,subprocess,shutil
repo=Path('/workspace/symaira-doctor765-json');source='280914bc6bc9c3b3faac404dcd5d84856a424ed7';prefix='symaira-doctor765-json-final';dest=repo/'migration/evidence/doctor-windows-765/final-280';dest.mkdir(exist_ok=True)
sha=lambda data:hashlib.sha256(data).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==source
assert subprocess.check_output(['git','status','--porcelain'],cwd=repo,text=True).strip() in ('','?? migration/evidence/doctor-windows-765/final-280/')
cli=Path('/workspace/symaira-setup765-source/target/debug/symbrain');binary_sha=sha(cli.read_bytes());guard_cli=cli.with_name('symguard')
shutil.copyfile(cli,'/tmp/symaira-doctor765-json-280-cli')
artifacts={};logs={};gates={};controls=[]
def retain(path):
 path=Path(path);raw=path.read_bytes();shutil.copyfile(path,dest/path.name);artifacts[path.name]=sha(raw)
for family,total in [('source',111),('doctor',285),('setup',201)]:
 p=Path('/tmp')/(prefix+'-'+family+'.json');r=json.loads(p.read_bytes());assert r['candidate_head']==source and not r['candidate_dirty'] and r['total']==total and r['exit']==0
 assert r['rust_binary_sha256']==binary_sha and not r.get('failures')
 assert (r.get('matched')==total if family!='setup' else r['matches'] is True)
 for field in ['candidate_source_sha256','go_source_sha256']:
  for name,digest in r[field].items():
   raw=(repo/name).read_bytes() if field.startswith('candidate') else subprocess.check_output(['git','show',r['go_oracle_ref']+':'+name],cwd=repo)
   assert sha(raw)==digest,(family,field,name)
 gates[family]=dict(total=total,all_matching=True,actual_native_platform='Linux',candidate_source_entries=len(r['candidate_source_sha256']),go_source_entries=len(r['go_source_sha256']),go_binary_sha256=r['go_binary_sha256']);retain(p)
for family,modes in [('source',['wrong-exit','wrong-source']),('doctor',['wrong-exit','missing-header','missing-core-event'])]:
 for mode in modes:
  p=Path('/tmp')/(prefix+'-'+family+'.json.'+mode+'.json');r=json.loads(p.read_bytes());field={'wrong-exit':'exit','wrong-source':'stdout_base64','missing-header':'stdout_base64','missing-core-event':'logs'}[mode]
  assert r['candidate_head']==source and not r['candidate_dirty'] and r['rust_binary_sha256']==binary_sha
  assert r['total']==r['complete_observations']==1 and r['matched']==0 and r['exit']==1 and r['failures']==[{'case':'explicit-browse-json' if family=='source' else 'correct','fields':[field]}]
  controls.append(dict(family=family,mode=mode,exact_intended_failure=r['failures']));retain(p)
counts={}
for name,fields in [('findings',['source_cases','doctor_cases']),('lookup',['observations']),('raw-signals',['raw_argv_observations']),('eight-extra',['cases']),('eight-home',['cases']),('boundaries',['cases']),('symlink',['cases'])]:
 p=Path('/tmp')/(prefix+'-'+name+'.json');r=json.loads(p.read_bytes());assert r.get('candidate_head',r.get('head'))==source and not r.get('candidate_dirty',r.get('dirty',False))
 assert r.get('rust_sha256',r.get('rust_sha',r.get('rust_binary_sha256')))==binary_sha
 rows=[row for field in fields for row in r[field]];counts[name]=len(rows)
 for row in rows:assert row.get('mismatched_fields',row.get('differences'))==[],(name,row['case'])
 if name=='raw-signals':
  assert [v['signal'] for v in r['actual_cancellation']]==['SIGINT','SIGTERM']
  assert all(not v['descendant_active'] and v['actual_cli_exit']==1 for v in r['actual_cancellation'])
 retain(p);retain(p.with_suffix('.py'))
assert counts==dict(findings=11,lookup=3,**{'raw-signals':6,'eight-extra':6,'eight-home':2},boundaries=6,symlink=1)
consumer_counts={}
for name,total in [('consumers-setup',8),('fix-consumers-setup',8),('consumers-source',4),('consumers-doctor',3)]:
 p=Path('/tmp')/(prefix+'-'+name+'.json');r=json.loads(p.read_bytes())
 if name.endswith('setup'):
  assert r['candidate_head']==source and not r['candidate_dirty'] and r['total']==total and r['exit']==0 and r['matches']
  assert r['rust_binary_sha256']==binary_sha
 else:
  assert r['head']==source and not r['dirty'] and r['rust_sha']==binary_sha and len(r['cases'])==total
  assert all(not row['differences']for row in r['cases'])
 consumer_counts[name]=total;retain(p)
for name in ['consumers','fix-consumers']:retain(Path('/tmp')/(prefix+'-'+name+'.py'))
guard=json.loads(Path('/tmp/'+prefix+'-guard.json').read_bytes());assert guard['candidate_head']==source and not guard['candidate_dirty'] and guard['total']==124 and guard['matched']==121
assert len(guard['remaining_native_diagnostic_states'])==3 and not guard['audit_diagnostic_deviations']
for field in ['candidate_source_sha256','candidate_manifest_sha256']:
 for name,digest in guard[field].items():assert sha((repo/name).read_bytes())==digest
raw=json.loads(Path('/tmp/'+prefix+'-guard-raw-paths.json').read_bytes());assert raw['candidate_head']==source and not raw['candidate_dirty'] and raw['total']==raw['matched']==63
gc=json.loads(Path('/tmp/'+prefix+'-guard-controls.json').read_bytes());assert gc['candidate_head']==source and not gc['candidate_dirty'] and gc['rejected']==3
assert len(raw['controls'])==2 and all(c['rejected'] for c in raw['controls'])
for name in ['guard','guard-controls','guard-raw-paths']:retain(Path('/tmp')/(prefix+'-'+name+'.json'))
win=json.loads(Path('/tmp/'+prefix+'-windows-managed-join.json').read_bytes());generic=json.loads(Path('/tmp/'+prefix+'-windows-lexical.json').read_bytes())
assert win['head']==generic['head']==source and win['cases']==generic['cases']==189679
assert win['differences']==[dict(input_hex='',sdk_hex='2e73796d616972615c62696e',rust_hex='5c2e73796d616972615c62696e')]
assert len(generic['differences'])==101
home=(repo/'rust/symbrain-cli/src/managed_home.rs').read_bytes();assert win['actual_source_sha256']==generic['actual_source_sha256']==sha(home)
assert b'var_os(HOME_VARIABLE).filter(|home| !home.is_empty())?'in home
for name in ['windows-managed-join','windows-lexical']:
 retain(Path('/tmp')/(prefix+'-'+name+'.json'));retain(Path('/tmp')/(prefix+'-'+name+'.py'))
summaries=[tuple(map(int,v))for v in re.findall(rb'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',Path('/tmp/'+prefix+'-tests.log').read_bytes())]
assert len(summaries)==48 and tuple(map(sum,zip(*summaries)))==(451,0,0)
executables={}
for name in re.findall(r'Running [^\n]+ \(([^\n]+)\)',Path('/tmp/'+prefix+'-tests.log').read_text()):
 p=Path(name);executables[name]=dict(sha256=sha(p.read_bytes()),bytes=p.stat().st_size)
for p in [cli,guard_cli]:executables[str(p)]=dict(sha256=sha(p.read_bytes()),bytes=p.stat().st_size)
for p in sorted(Path('/tmp').glob(prefix+'-*.log')):
 raw=p.read_bytes();logs[p.name]=dict(sha256=sha(raw),original_bytes_base64=base64.b64encode(raw).decode())
logs['initial-tests']={ 'sha256':sha(Path('/tmp/symaira-doctor765-json-initial-tests.log').read_bytes()),'original_bytes_base64':base64.b64encode(Path('/tmp/symaira-doctor765-json-initial-tests.log').read_bytes()).decode()}
logs['static-win-first']={ 'sha256':sha(Path('/tmp/symaira-doctor765-json-static-win-first.log').read_bytes()),'original_bytes_base64':base64.b64encode(Path('/tmp/symaira-doctor765-json-static-win-first.log').read_bytes()).decode()}
for name in ['clippy','static-win','static-mac']:assert b'Finished' in Path('/tmp/'+prefix+'-'+name+'.log').read_bytes()
for name in ['fmt','actionlint']:assert not Path('/tmp/'+prefix+'-'+name+'.log').read_bytes()
assert Path('/tmp/'+prefix+'-gates.log').read_text().strip()==source
(dest/'logs.json').write_text(json.dumps(logs,indent=2)+'\n');artifacts['logs.json']=sha((dest/'logs.json').read_bytes())
harness=Path('/tmp/doctor765-json-typecheck');typed=dict(scope='Only all-target type/lint with explicit surrounding signature stubs; actual Source/Doctor, managed owner/mkdir/local install, new shared Core GoText/json primitive and actual Setup shared escape helper. Entire Setup CLI tested on Linux. No linking or Windows/macOS runtime. CARGO_BIN_EXE_symbrain is an explicitly unexecuted type-check placeholder.',env=dict(CARGO_BIN_EXE_symbrain='/not-executed/typecheck-only-symbrain',CARGO_TARGET_DIR='/tmp/source765-typecheck-r2ubksmn/target'),files={str(p.relative_to(harness)):dict(sha256=sha(p.read_bytes()),text=p.read_text())for p in [harness/'Cargo.toml',harness/'Cargo.lock',harness/'src/lib.rs']})
(dest/'signature-stub-harness.json').write_text(json.dumps(typed,indent=2)+'\n');artifacts['signature-stub-harness.json']=sha((dest/'signature-stub-harness.json').read_bytes())
original=repo/'migration/evidence/doctor-windows-765/independent-review-a66';ret=json.loads((original/'retention.json').read_bytes())
for name,item in ret['original_files'].items():
 p=original/item['retained'];raw=p.read_bytes() if item['encoding']=='literal' else base64.b64decode(json.loads(p.read_bytes())['original_bytes_base64']);assert sha(raw)==item['sha256'] and len(raw)==item['bytes'],name
archive=json.loads((original/'binary-retention-a66.json').read_bytes());assert len(archive['records'])==38
for item in archive['records']:
 p=Path(item['archive']);assert sha(p.read_bytes())==item['archive_sha256'];raw=gzip.decompress(p.read_bytes());assert sha(raw)==item['sha256'] and len(raw)==item['bytes']
old=json.loads((repo/'migration/evidence/doctor-windows-765/final-a66/validation.json').read_bytes());named={}
for family in ['source','doctor','setup']:
 r=json.loads((repo/'migration/evidence/doctor-windows-765/final-a66'/('symaira-doctor765-owner-a66-'+family+'.json')).read_bytes());n=json.loads((dest/(prefix+'-'+family+'.json')).read_bytes())
 left=set(r['cases']if family=='setup'else[row['case']for row in r['observations']]);right=set(n['cases']if family=='setup'else[row['case']for row in n['observations']]);assert left<=right
 named[family]=dict(previous=len(left),current=len(right),all_names_retained=True)
for ancestor in ['34394496b44d0f95581080885d89d4ed5f43bbd5','e3dbda6cbb95429237d15a9b176209b7d107792c','8e3b21b4fa649847f5b9a26a80f0c24fa172e1a0']:subprocess.run(['git','merge-base','--is-ancestor',ancestor,source],cwd=repo,check=True)
changed=subprocess.check_output(['git','diff','--name-only','34394496b44d0f95581080885d89d4ed5f43bbd5',source],cwd=repo,text=True).splitlines();assert not[p for p in changed if p.endswith('.go')or p.startswith('rust/symbrain-cli/tests/fixtures/')]
receipt=dict(source_head=source,reviewed_previous_source='a66b4818f74943812315509a3ba2545b2b6b2ea1',reviewed_previous_publication='34394496b44d0f95581080885d89d4ed5f43bbd5',normal_main='e3dbda6cbb95429237d15a9b176209b7d107792c',independently_approved_shared_Core_Guard='8e3b21b4fa649847f5b9a26a80f0c24fa172e1a0',rust_binary_sha256=binary_sha,native_cli_path=str(cli),preserved_actual_cli='/tmp/symaira-doctor765-json-280-cli',tests=dict(passed=451,failed=0,ignored=0,summaries=48,initial_cli_managed_core=388,normal_integrated_guard_additional=63),fresh_process_gates=gates,actual_negative_controls=controls,all35_original_pairs=counts,actual_cancellation_signals=['SIGINT','SIGTERM'],all23_third_review_consumer_pairs=consumer_counts,eight_original_json_failures_closed=True,eight_original_human_controls_unchanged=True,guard_regressions=dict(cases=124,full_matches=121,approved_retained_states=3,raw_cases=63,controls_rejected=5),actual_test_cli_executables=executables,artifacts=artifacts,original_third_review_files_verified=len(ret['original_files']),original_third_reviewed_executables_roundtrip_verified=38,prior46_35_24_maps_preserved=True,original_named_corpus_retention=named,windows_actual_assembly=dict(total_inputs=189679,admitted_nonempty_home=189678,admitted_mismatches=0,one_empty_home_precondition_difference_retained=True,generic101_exploratory_failures_retained=True,scope='Actual extracted Windows SDK and actual managed-path assembly hosted on Linux; no Windows OS runtime or filesystem proof'),candidate_clean_during_all_final_gates=True,native_platforms='Actual Linux only; Windows95Source/269Doctor/177Setup and macOS111/285/201 require exact-head native CI. Signature stubs are not native execution.',worker_ownership='Historical Go Browse/Swift workers remain transitional, broad typed config diagnostics remain Go-owned; no full765/783 cutover.',environment=dict(cwd=str(repo),subreaper='/tmp/symaira-subreaper.py',shell='set -euo pipefail',umask='022',rust_env='/home/agent/.cargo/env',PATH_prefix='/workspace/toolchains/go1.26.7/bin',CARGO_TARGET_DIR='/workspace/symaira-setup765-source/target',CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0'),disposition='All exact-source local gates pass; another full independent review and exact native three-OS CI required before remote publication or merge.')
(dest/'validation.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(dict(source=source,cli=binary_sha,tests=receipt['tests'],actual_executables=len(executables),third_consumer_pairs=consumer_counts)))
