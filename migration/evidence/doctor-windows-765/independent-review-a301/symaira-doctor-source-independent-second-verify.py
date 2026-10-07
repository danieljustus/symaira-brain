from pathlib import Path
import base64,hashlib,json,os,re,subprocess
repo=Path('/workspace/symaira-doctor765-windows');sha=lambda b:hashlib.sha256(b).hexdigest()
head='9705777b09bbfe600406ad60ba704d0fd8759e48';source='a301969596358fb42e3bd93972c315bf89e79963';prefix='/tmp/symaira-doctor-source-independent-second-'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head
assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
assert not subprocess.check_output(['git','diff',source,head,'--','rust','scripts','.github','Cargo.toml','Cargo.lock'],cwd=repo)
subprocess.run(['git','merge-base','--is-ancestor','31de72294521701fc4b1a0bce39f03cc34d72e7e',head],cwd=repo,check=True)
controls=[];gates={};manifests={}
for family,expected,modes in [('source',103,['wrong-exit','wrong-source']),('doctor',274,['wrong-exit','missing-header','missing-core-event']),('setup',151,[])]:
 path=Path(prefix+family+'.json');data=json.loads(path.read_bytes())
 assert not data['candidate_dirty'] and data['candidate_head']==head
 if family!='setup':assert data['total']==data['complete_observations']==data['matched']==expected and data['exit']==0 and not data['failures']
 else:assert data['total']==expected and data['matches'] is True and len(data['cases'])==expected and len(data['observations'])==2*expected and data['exit']==0
 manifests[family]={}
 for key in ['candidate_source_sha256','go_source_sha256']:
  if key not in data:continue
  manifests[family][key]=len(data[key])
  for name,digest in data[key].items():
   actual=(repo/name).read_bytes() if key=='candidate_source_sha256' else subprocess.check_output(['git','show',f'dcddcef0df5789123c7c9a7ebe6e01f10e941f2c:{name}'],cwd=repo)
   assert sha(actual)==digest,(family,name)
 gates[family]=dict(total=expected,path=str(path),sha256=sha(path.read_bytes()))
 for mode in modes:
  p=Path(str(path)+'.'+mode+'.json');q=json.loads(p.read_bytes());field={'wrong-exit':'exit','wrong-source':'stdout_base64','missing-header':'stdout_base64','missing-core-event':'logs'}[mode]
  assert q['total']==q['complete_observations']==1 and q['matched']==0 and q['exit']==1
  assert q['failures']==[{'case':'explicit-browse-json' if family=='source' else 'correct','fields':[field]}]
  controls.append(dict(family=family,mode=mode,path=str(p),sha256=sha(p.read_bytes()),intended_failure=q['failures']))
extra=json.loads(Path(prefix+'extra.json').read_bytes());home=json.loads(Path(prefix+'home.json').read_bytes())
assert len(extra['cases'])+len(home['cases'])==8 and all(not row['differences'] for row in extra['cases']+home['cases'])
boundaries=json.loads(Path(prefix+'boundaries.json').read_bytes());symlink=json.loads(Path(prefix+'symlink.json').read_bytes())
assert len(boundaries['cases'])==6 and len(symlink['cases'])==1
assert sum(bool(r['differences']) for r in boundaries['cases']+symlink['cases'])==6
originals=[json.loads(Path(prefix+name+'.json').read_bytes()) for name in ['original-raw-signals','original-findings','original-lookup']]
original_count=0
for doc in originals:
 for value in doc.values():
  if isinstance(value,list):
   for row in value:
    if isinstance(row,dict) and 'mismatched_fields' in row:
     assert not row['mismatched_fields'],row
     original_count+=1
assert original_count==20
assert len(originals[0]['actual_cancellation'])==2 and all(not row['descendant_active'] and row['actual_cli_exit']==1 for row in originals[0]['actual_cancellation'])
log=Path(prefix+'tests.log').read_bytes();summaries=[tuple(map(int,x)) for x in re.findall(rb'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)]
assert tuple(map(sum,zip(*summaries)))==(399,0,0) and len(summaries)==40
actual=Path('/workspace/symaira-setup765-source/target/debug/symbrain');assert sha(actual.read_bytes())=='44ffc3c6ecd498aa29aa82dd828abeeef161ca073f14990ed695b9fb9c681b80'
changed=subprocess.check_output(['git','diff','--name-only','f1623bfb5aaac143d6783f7683ce70e5443822ae',source,'--','.',':!migration/evidence'],cwd=repo,text=True).splitlines()
inputs={name:sha((repo/name).read_bytes()) for name in changed if (repo/name).is_file()}
artifact_hashes={str(p):sha(p.read_bytes()) for p in sorted(Path('/tmp').glob('symaira-doctor-source-independent-second-*')) if p.is_file() and p.suffix in ('.py','.json','.log') and 'review-receipt' not in p.name}
users=[];target='/workspace/symaira-setup765-source/target'
for p in Path('/proc').iterdir():
 if not p.name.isdigit() or int(p.name)==os.getpid():continue
 try:
  exe=os.readlink(p/'exe');cwd=os.readlink(p/'cwd');fds=[os.readlink(fd) for fd in (p/'fd').iterdir()]
  if any(s.startswith(target) for s in [exe,cwd,*fds]):users.append(dict(pid=p.name,exe=exe,cwd=cwd))
 except (FileNotFoundError,PermissionError,ProcessLookupError):pass
assert not users,users
report=dict(head=head,validated_source=source,original_reviewed_source='0b56d4edc7f9e6728fdbf955df89c06e2a1689d2',source_target_unchanged=True,evidence_only_publication=True,inputs=inputs,gates=gates,controls=controls,manifest_counts=manifests,fresh_tests=dict(passed=399,failed=0,ignored=0,complete_summaries=40),strict_clippy=True,fmt=True,actionlint_all_workflows=True,original_eight_pairs_match=True,original_twenty_pairs_match=True,actual_SIGINT_SIGTERM_match=True,new_extra_pairs=7,new_extra_mismatches=6,new_positive_controls=1,rust_sha256=sha(actual.read_bytes()),artifacts=artifact_hashes,provenance=json.loads(Path(prefix+'provenance.json').read_bytes()),original_archives=json.loads(Path(prefix+'archives.json').read_bytes()),native_windows_macos_runtime_verified=False,target_users=users,disposition='REQUEST CHANGES: two remaining Doctor P2 groups; original three groups closed for tested inputs',reviewer_execution_notes=['Initial Setup launcher used nonexistent scripts/setup-oracle/run.sh; its exit127 log is retained. Only Setup was then run at correct setup-repair-oracle launcher; no exception counts as a control.','Receipt parsing was corrected for documented Setup matches/cases/observations structure and encoded historical log containers; no process observations were changed or rerun.','The reused original lookup runner retains historical source_head=0b56 metadata; its actual candidate_head=970 and CLI SHA44 are verified separately. Runtime fixture inputs are unchanged.'])
Path('/tmp/symaira-doctor-source-independent-second-review-receipt.json').write_bytes((json.dumps(report,indent=2)+'\n').encode())
print(json.dumps({k:report[k] for k in ['manifest_counts','fresh_tests','original_eight_pairs_match','new_extra_pairs','new_extra_mismatches','target_users']},indent=2))
