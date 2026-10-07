from pathlib import Path
import base64,gzip,hashlib,json,re,subprocess,shutil,zipfile
repo=Path('/workspace/symaira-doctor765-windows');source='a301969596358fb42e3bd93972c315bf89e79963';prefix='symaira-doctor765-home-a301';dest=repo/'migration/evidence/doctor-windows-765/final-a301';dest.mkdir()
sha=lambda data:hashlib.sha256(data).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==source
# The only newly created content is this receipt directory, still empty.
assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
cli=Path('/workspace/symaira-setup765-source/target/debug/symbrain');binary_sha=sha(cli.read_bytes());assert sha(Path('/tmp/symaira-doctor765-home-a301-cli').read_bytes())==binary_sha
artifacts={};logs={};gates={};controls=[]
def retain(path):
 path=Path(path);data=path.read_bytes()
 shutil.copyfile(path,dest/path.name);artifacts[path.name]=sha(data)
for family,total in [('source',103),('doctor',274),('setup',151)]:
 p=Path('/tmp')/(prefix+'-'+family+'.json');r=json.loads(p.read_bytes());assert r['candidate_head']==source and not r['candidate_dirty'] and r['total']==total and r['exit']==0
 assert r['rust_binary_sha256']==binary_sha and not r.get('failures')
 assert (r.get('matched')==total if family!='setup' else r['matches'] is True)
 for field in ['candidate_source_sha256','go_source_sha256']:
  for name,digest in r[field].items():
   data=(repo/name).read_bytes() if field.startswith('candidate') else subprocess.check_output(['git','show',r['go_oracle_ref']+':'+name],cwd=repo)
   assert sha(data)==digest,(family,field,name)
 gates[family]={'total':total,'all_matching':True,'actual_native_platform':'Linux','go_binary_sha256':r['go_binary_sha256']};retain(p)
for family,modes in [('source',['wrong-exit','wrong-source']),('doctor',['wrong-exit','missing-header','missing-core-event'])]:
 for mode in modes:
  p=Path('/tmp')/(prefix+'-'+family+'.json.'+mode+'.json');r=json.loads(p.read_bytes());field={'wrong-exit':'exit','wrong-source':'stdout_base64','missing-header':'stdout_base64','missing-core-event':'logs'}[mode]
  assert r['candidate_head']==source and not r['candidate_dirty'] and r['rust_binary_sha256']==binary_sha
  assert r['total']==r['complete_observations']==1 and r['matched']==0 and r['exit']==1 and r['failures']==[{'case':'explicit-browse-json' if family=='source' else 'correct','fields':[field]}]
  controls.append({'family':family,'mode':mode,'exact_intended_failure':r['failures']});retain(p)
counts={}
for name,fields in [('findings',['source_cases','doctor_cases']),('lookup',['observations']),('raw-signals',['raw_argv_observations']),('eight-extra',['cases']),('eight-home',['cases'])]:
 p=Path('/tmp')/('symaira-doctor765-home-final-'+name+'.json');r=json.loads(p.read_bytes());assert r.get('candidate_head',r.get('head'))==source and not r.get('candidate_dirty',r.get('dirty',False))
 assert r.get('rust_sha256',r.get('rust_sha',r.get('rust_binary_sha256')))==binary_sha
 rows=[row for field in fields for row in r[field]];counts[name]=len(rows)
 for row in rows:assert row.get('mismatched_fields',row.get('differences'))==[],(name,row['case'])
 if name=='raw-signals':
  assert [v['signal'] for v in r['actual_cancellation']]==['SIGINT','SIGTERM']
  assert all(not v['descendant_active'] and v['actual_cli_exit']==1 for v in r['actual_cancellation'])
 retain(p);retain(p.with_suffix('.py'))
assert counts=={'findings':11,'lookup':3,'raw-signals':6,'eight-extra':6,'eight-home':2}
s=Path('/tmp')/(prefix+'-tests.log');summaries=[tuple(map(int,v)) for v in re.findall(rb'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',s.read_bytes())]
assert len(summaries)==40 and tuple(map(sum,zip(*summaries)))==(399,0,0)
executables={}
for name in re.findall(r'Running [^\n]+ \(([^\n]+)\)',s.read_text()):
 p=Path(name);executables[name]={'sha256':sha(p.read_bytes()),'bytes':p.stat().st_size}
executables[str(cli)]={'sha256':binary_sha,'bytes':cli.stat().st_size};assert len(executables)==38
for p in sorted(Path('/tmp').glob(prefix+'-*.log')):
 data=p.read_bytes();logs[p.name]={'sha256':sha(data),'original_bytes_base64':base64.b64encode(data).decode()}
assert Path('/tmp/'+prefix+'-gates.log').read_text().strip()==source
for name in ['clippy','build','static-win','static-mac-installed-target']:
 assert b'Finished' in (Path('/tmp')/(prefix+'-'+name+'.log')).read_bytes()
for name in ['fmt','actionlint']:assert not (Path('/tmp')/(prefix+'-'+name+'.log')).read_bytes()
(dest/'logs.json').write_text(json.dumps(logs,indent=2)+'\n');artifacts['logs.json']=sha((dest/'logs.json').read_bytes())
harness=Path('/tmp/doctor765-home-typecheck-pg1d_k3w');typed={'scope':'Only all-target type/lint checks with explicit surrounding signature stubs; actual Source/Doctor/log/home-test/local-install/provenance-reader code. No linking or native Windows/macOS execution. Initial aarch64 Darwin check failed because SDK not installed; corrected x86_64 Darwin and Windows GNU checks pass. Full native CI required.','commands':['cargo clippy --target x86_64-pc-windows-gnu --all-targets --locked -- -D warnings','cargo clippy --target x86_64-apple-darwin --all-targets --locked -- -D warnings'],'files':{name:{'sha256':sha((harness/name).read_bytes()),'text':(harness/name).read_text()} for name in ['Cargo.toml','Cargo.lock','src/lib.rs']}}
(dest/'signature-stub-harness.json').write_text(json.dumps(typed,indent=2)+'\n');artifacts['signature-stub-harness.json']=sha((dest/'signature-stub-harness.json').read_bytes())
ret=repo/'migration/evidence/doctor-windows-765/independent-review-0b56';r=json.loads((ret/'retention.json').read_bytes())
for name,item in r['original_files'].items():
 p=ret/name
 data=p.read_bytes() if p.exists() else base64.b64decode(json.loads((ret/(name+'.json')).read_bytes())['original_bytes_base64'])
 assert sha(data)==item['sha256'] and len(data)==item['bytes'],name
archive=json.loads((ret/'binary-retention-0b56.json').read_bytes());assert archive['source_head']=='0b56d4edc7f9e6728fdbf955df89c06e2a1689d2'
for name,item in archive['executables'].items():
 p=Path(item['gzip_archive']);assert sha(p.read_bytes())==item['archive_sha256'];data=gzip.decompress(p.read_bytes());assert sha(data)==item['original_sha256'] and len(data)==item['original_bytes'],name
assert len(archive['executables'])==41
old=repo/'migration/evidence/setup-source-765/independent-review-86ea';r86=json.loads((old/'retention.json').read_bytes())
for name,item in r86['originals'].items():
 p=old/name;data=p.read_bytes() if p.exists() else base64.b64decode(json.loads((old/(name+'.json')).read_bytes())['original_bytes_base64']);assert sha(data)==item['sha256'],name
windows=repo/'migration/evidence/doctor-windows-765/original-windows-1288';w=json.loads((windows/'receipt.json').read_bytes())
for name,item in w.items():
 if not isinstance(item,dict) or 'sha256' not in item:continue
 p=windows/item.get('stored_path',name);data=base64.b64decode(json.loads(p.read_bytes())['original_bytes_base64']) if item.get('stored_encoding') else p.read_bytes();assert sha(data)==item['sha256'],name
original=json.loads((windows/'windows-process.json').read_bytes());five=json.loads((windows/'five-full-failure-records.json').read_bytes());assert len(five['failures'])==5 and five['failures']==original['failures'] and five['observations']==[x for x in original['observations'] if not x['matches']]
with zipfile.ZipFile(windows/'doctor-windows-failure.zip') as z:assert any(z.read(name)==(windows/'windows-process.json').read_bytes() for name in z.namelist() if name.endswith('.json'))
changed=subprocess.check_output(['git','diff','--name-only','f1623bfb5aaac143d6783f7683ce70e5443822ae',source],cwd=repo,text=True).splitlines();assert not [p for p in changed if p.endswith('.go') or p.startswith('rust/symbrain-cli/tests/fixtures/')]
receipt={'source_head':source,'reviewed_previous_source':'0b56d4edc7f9e6728fdbf955df89c06e2a1689d2','reviewed_previous_publication':'f1623bfb5aaac143d6783f7683ce70e5443822ae','oracle_ref':'dcddcef0df5789123c7c9a7ebe6e01f10e941f2c','rust_binary_sha256':binary_sha,'native_cli_path':str(cli),'preserved_actual_cli':'/tmp/symaira-doctor765-home-a301-cli','tests':{'passed':399,'failed':0,'ignored':0,'summaries':40},'fresh_process_gates':gates,'actual_negative_controls':controls,'actual_review_failed_inputs_rerun_all_match':counts,'actual_cancellation_signals':['SIGINT','SIGTERM'],'actual_test_cli_executables':executables,'artifacts':artifacts,'original0b56_review_files_verified':len(r['original_files']),'original0b56_reviewed_executables_gzip_verified':41,'original86_review_files_verified':len(r86['originals']),'original_windows_exact_zip_process_and_five_failures_verified':True,'candidate_clean_during_all_final_process_gates':True,'platform_scope':'Actual Linux CLI/process/tests only. Windows264 Doctor/92Source, macOS274Doctor/103Source and Setup151 native CI still required. Typed stub harness does not establish native runtime or full CLI build.','worker_ownership':'Historical Go Browse and Swift workers remain transitional. Typed full invalid configuration still Go-owned. No full765/783 closure or Go-independent source installation.','environment':{'cwd':str(repo),'subreaper':'python3 /tmp/symaira-subreaper.py bash -c','shell':'set -euo pipefail','umask':'022','rust_env':'/home/agent/.cargo/env','PATH_prefix':'/workspace/toolchains/go1.26.7/bin','CARGO_TARGET_DIR':'/workspace/symaira-setup765-source/target','CARGO_INCREMENTAL':'0','CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0'},'disposition':'All source-exact local gates pass. Full independent read-only review and actual native three-OS CI required before remote publication/merge.'}
(dest/'validation.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({k:receipt[k] for k in ['source_head','rust_binary_sha256','tests','actual_review_failed_inputs_rerun_all_match','original0b56_review_files_verified','original0b56_reviewed_executables_gzip_verified']}))
