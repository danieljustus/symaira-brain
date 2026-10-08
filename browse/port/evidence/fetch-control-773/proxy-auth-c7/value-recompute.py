from pathlib import Path
import hashlib,json,math,statistics,sys
sys.path.insert(0,'/workspace/symaira-fetch773-proxy-auth/browse/port/harness')
import fetch_control_process as common
prefix='/tmp/symaira-fetch773-proxy-auth-clean-'
sha=lambda path:hashlib.sha256(Path(path).read_bytes()).hexdigest()
report=json.loads(Path(prefix+'quiet-paired-30.json').read_text());comparison=json.loads(Path(prefix+'quiet-comparison.json').read_text());load=json.loads(Path(prefix+'quiet-load-receipt.json').read_text());cost=json.loads(Path(prefix+'route-cost.json').read_text())
source='c7beb3a7a39dbc10f7e84769fccea5cf20e7f09e'
assert report['source_revision']==load['candidate_source']==cost['candidate_source']==source
assert not load['candidate_dirty'] and not cost['candidate_dirty']
assert report['runs_per_workload']==30 and report['gate']=='pass' and comparison['gate']=='pass'
assert load['report_sha256']==sha(prefix+'quiet-paired-30.json') and load['comparator_sha256']==sha(prefix+'quiet-comparison.json')
sets=[]
for side in ['go','rust']:
 identity=report['binaries'][side]['identity'];assert sha(identity['path'])==identity['sha256']==load['binary_sha256'][side]
 build=json.loads(Path(prefix+side+'-build.json').read_text());assert build['binary_sha256']==identity['sha256'];assert build['source_clean']
 for workload in report['workloads']:
  row=report['binaries'][side][workload];assert row['status']=='pass' and row['samples']==len(row['raw_samples'])==30 and row['statuses']==['pass']*30
  values=[x['duration_ns'] for x in row['raw_samples']];p95=sorted(values)[math.ceil(.95*30)-1];median=int(statistics.median(values))
  assert p95==row['p95_duration_ns'] and median==row['median_duration_ns']
  if workload=='fetch':assert row['semantic_contract']['negative_control']['rejected']
  sets.append(dict(side=side,workload=workload,samples=30,p95_ns=p95,median_ns=median))
for workload in report['workloads']:
 go=report['binaries']['go'][workload]['p95_duration_ns'];rust=report['binaries']['rust'][workload]['p95_duration_ns'];change=(rust/go-1)*100
 assert abs(change-comparison['workloads'][workload]['p95_change_percent'])<1e-9 and change<=10
summary=[]
for group in cost['groups']:
 assert group['matched']==len(group['cases'])==30
 assert all(common.comparable(a)==common.comparable(b) for a,b in zip(group['go']['records'],group['rust']['records']))
 for side in ['go','rust']:
  row=group[side];assert len(row['records'])==len(row['response_arrival_ms'])==len(row['response_interarrival_ms'])==30
  assert row['response_interarrival_ms'][0]==row['response_arrival_ms'][0]
  for i in range(1,30):assert abs(row['response_interarrival_ms'][i]-(row['response_arrival_ms'][i]-row['response_arrival_ms'][i-1]))<1e-9
  assert row['exit']==0
  summary.append(dict(scheme=group['proxy_scheme'],port=group['target_port'],side=side,connections=row['accepted_connections'],later_response_median_ms=statistics.median(row['response_interarrival_ms'][1:])))
result=dict(source=source,samples_recomputed=240,sets=sets,load_observations=len(load['observations']),load_receipt_sha256=sha(prefix+'quiet-load-receipt.json'),current_size_bytes={side:report['binaries'][side]['identity']['size_bytes'] for side in ['go','rust']},historical_size_reference='Unchanged comparator uses recorded Darwin-arm64 release baseline18,330,466B; current Go Linux release is separately26,611,127B.',unchanged_comparator=comparison,cost_pairs=120,cost=summary,scope='Author recomputation, not independent approval; no sample exclusions or threshold/reference edits.')
Path(prefix+'value-recomputed.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({'samples':240,'pairs':120,'gate':comparison['gate'],'cost':summary},indent=2))
