from pathlib import Path
import base64,hashlib,json,re,subprocess,zipfile
repo=Path('/workspace/symaira-doctor765-windows');sha=lambda b:hashlib.sha256(b).hexdigest()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo).decode().strip();source='0b56d4edc7f9e6728fdbf955df89c06e2a1689d2';base='31de72294521701fc4b1a0bce39f03cc34d72e7e'
assert head=='f1623bfb5aaac143d6783f7683ce70e5443822ae'
assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
assert not subprocess.check_output(['git','diff',source,head,'--','rust','scripts','.github','.gitattributes','Cargo.lock'],cwd=repo)
subprocess.run(['git','merge-base','--is-ancestor',base,head],cwd=repo,check=True)
changed=subprocess.check_output(['git','diff','--name-only',base,head],cwd=repo).decode().splitlines(); inputs={p:sha((repo/p).read_bytes()) for p in changed if not p.startswith('migration/evidence/')}
assert not [p for p in changed if p.endswith('.go') or p.startswith('rust/symbrain-cli/tests/fixtures/')], 'immutable production/fixtures changed'
report_hashes={};gates={};manifest_counts={}
for family in ('source','doctor','setup'):
 path=Path('/tmp/symaira-doctor-source-independent-'+family+'.json');q=json.loads(path.read_bytes());matched=q.get('matched',q['total'] if q.get('matches') is True else -1);assert q['total']==matched and not q.get('failures') and not q.get('candidate_dirty')
 gates[family]={'total':q['total'],'matched':matched,'head':q.get('candidate_head'),'rust_binary_sha256':q['rust_binary_sha256'],'go_binary_sha256':q['go_binary_sha256']}
 report_hashes[str(path)]=sha(path.read_bytes());count={}
 for field in ('candidate_source_sha256','go_source_sha256'):
  if field not in q:continue
  for name,digest in q[field].items():
   actual=(repo/name).read_bytes() if field.startswith('candidate') else subprocess.check_output(['git','show',q['go_oracle_ref']+':'+name],cwd=repo)
   assert sha(actual)==digest,(family,name)
  count[field]=len(q[field])
 manifest_counts[family]=count
controls=[]
for family,modes in [('source',['wrong-exit','wrong-source']),('doctor',['wrong-exit','missing-header','missing-core-event'])]:
 for mode in modes:
  p=Path('/tmp/symaira-doctor-source-independent-'+family+'.json.'+mode+'.json');q=json.loads(p.read_bytes());assert q['total']==q['complete_observations']==1 and q['matched']==0 and q['exit']==1
  expected={'wrong-exit':'exit','wrong-source':'stdout_base64','missing-header':'stdout_base64','missing-core-event':'logs'}[mode]
  assert q['failures']==[{'case':'explicit-browse-json' if family=='source' else 'correct','fields':[expected]}]
  controls.append({'family':family,'mode':mode,'path':str(p),'sha256':sha(p.read_bytes()),'exact_intended_failure':q['failures']})
final=repo/'migration/evidence/doctor-windows-765/final-0b56';author=json.loads((final/'validation.json').read_bytes());artifact_count=0
for name,digest in author['artifacts'].items():assert sha((final/name).read_bytes())==digest,name;artifact_count+=1
logs=json.loads((final/'logs.json').read_bytes())
for name,record in logs.items():assert sha(base64.b64decode(record['original_bytes_base64']))==record['sha256'],name
binary_counts=0
for name,item in author['actual_test_cli_executables'].items():assert sha(Path(name).read_bytes())==item['sha256'],name;binary_counts+=1
old=repo/'migration/evidence/setup-source-765/independent-review-86ea';retention=json.loads((old/'retention.json').read_bytes())
for name,item in retention['originals'].items():
 p=old/name
 data=p.read_bytes() if p.exists() else base64.b64decode(json.loads((old/(name+'.json')).read_bytes())['original_bytes_base64'])
 assert sha(data)==item['sha256'],name
windows=repo/'migration/evidence/doctor-windows-765/original-windows-1288';w=json.loads((windows/'receipt.json').read_bytes())
for name,item in w.items():
 if not isinstance(item,dict) or 'sha256' not in item:continue
 p=windows/item.get('stored_path',name)
 data=base64.b64decode(json.loads(p.read_bytes())['original_bytes_base64']) if item.get('stored_encoding') else p.read_bytes()
 assert sha(data)==item['sha256'],name
original=json.loads((windows/'windows-process.json').read_bytes());five=json.loads((windows/'five-full-failure-records.json').read_bytes())
assert len(five['failures'])==len(original['failures'])==5
assert five['failures']==original['failures']
assert five['observations']==[row for row in original['observations'] if not row['matches']]
with zipfile.ZipFile(windows/'doctor-windows-failure.zip') as archive:
 candidates=[name for name in archive.namelist() if name.endswith('.json')]
 assert any(archive.read(name)==(windows/'windows-process.json').read_bytes() for name in candidates),'ZIP process bytes differ'
testlog=Path('/tmp/symaira-doctor-source-independent-tests.log').read_bytes();summaries=[tuple(map(int,m)) for m in re.findall(rb'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',testlog)]
assert tuple(map(sum,zip(*summaries)))==(398,0,0)
for p in sorted(Path('/tmp').glob('symaira-doctor-source-independent*')):
 if p.is_file() and p.suffix in ('.py','.json','.log'):report_hashes[str(p)]=sha(p.read_bytes())
r={'head':head,'validated_source':source,'main_ancestor':base,'unchanged_candidate':True,'evidence_only_publication':True,'inputs':inputs,'fresh_gates':gates,'fresh_controls':controls,'fresh_tests':{'passed':398,'failed':0,'ignored':0,'complete_summaries':len(summaries)},'strict_clippy':True,'fmt':True,'actionlint':True,'manifest_counts':manifest_counts,'author_artifacts_verified':artifact_count,'author_logs_verified':len(logs),'author_actual_binaries_verified':binary_counts,'old_source86_failure_artifacts_verified':len(retention['originals']),'original_windows_exact_bytes_and_zip_verified':True,'windows_original_total':original['total'],'windows_original_matched':original['matched'],'windows_original_five_failures':five['failures'],'additional_actual_probes':{'new_pairs':8,'new_mismatches':6,'positive_controls':2,'original_replayed_pairs':20,'original_replayed_all_match':True,'actual_SIGINT_SIGTERM':True},'rust_sha256':sha(Path(author['native_cli_path']).read_bytes()),'sdk_go_sha256':sha(Path('/workspace/toolchains/go1.26.7/bin/go').read_bytes()),'SDK_windows_UserHomeDir_source_sha256':sha(Path('/workspace/toolchains/go1.26.7/src/os/file.go').read_bytes()),'artifacts':report_hashes,'disposition':'REQUEST CHANGES: three P2 groups, exact new raw probes retained. Actual Windows/macOS runtime and full765/783 still pending.'}
Path('/tmp/symaira-doctor-source-independent-review-receipt.json').write_bytes((json.dumps(r,indent=2)+'\n').encode());print(json.dumps({k:r[k] for k in ('fresh_tests','manifest_counts','author_artifacts_verified','author_logs_verified','author_actual_binaries_verified','old_source86_failure_artifacts_verified','original_windows_exact_bytes_and_zip_verified')},indent=2))
