from pathlib import Path
import json,hashlib,subprocess,statistics,math,sys
repo=Path('/workspace/symaira-fetch773-proxy-auth/browse');root=repo/'port/evidence/fetch-control-773/proxy-auth-c7';sys.path.insert(0,str(repo/'port/harness'));import fetch_control_process as common
read=lambda n:json.loads((root/n).read_text());sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest();report=read('quiet-paired-30.json');cost=read('route-cost.json');load=read('quiet-load-receipt.json');expected=read('quiet-comparison.json');source='c7beb3a7a39dbc10f7e84769fccea5cf20e7f09e'
assert report['source_revision']==cost['candidate_source']==load['candidate_source']==source
assert load['report_sha256']==sha(root/'quiet-paired-30.json') and load['comparator_sha256']==sha(root/'quiet-comparison.json')
assert not load['candidate_dirty'] and not cost['candidate_dirty']
assert set(report['workloads'])=={'cli','mcp','daemon','fetch'}
sets=[]
for side in ['go','rust']:
 identity=report['binaries'][side]['identity'];build=read(side+'-build.json');assert sha(identity['path'])==identity['sha256']==build['binary_sha256']==load['binary_sha256'][side]
 assert build['source_clean'];assert build['source_revision']==(source if side=='rust' else 'dcddcef0df5789123c7c9a7ebe6e01f10e941f2c')
 for name in report['workloads']:
  row=report['binaries'][side][name];samples=row['raw_samples'];assert len(samples)==row['samples']==30 and row['statuses']==['pass']*30 and row['status']=='pass'
  ns=[r['duration_ns'] for r in samples];assert all(isinstance(x,int) and x>0 for x in ns)
  p95=sorted(ns)[28];median=int(statistics.median(ns));assert p95==row['p95_duration_ns'] and median==row['median_duration_ns']
  sets.append(dict(side=side,workload=name,n=30,p95=p95,median=median))
  if name=='fetch':assert row['semantic_contract']['negative_control']['rejected'] is True
result=subprocess.run(['python3','port/bench/compare.py','docs/rust-port/baseline.json',str(root/'quiet-paired-30.json')],cwd=repo,text=True,capture_output=True);assert result.returncode==0,result.stderr
comparison=json.loads(result.stdout);assert comparison==expected
for name in report['workloads']:
 change=(report['binaries']['rust'][name]['p95_duration_ns']/report['binaries']['go'][name]['p95_duration_ns']-1)*100
 assert math.isclose(change,comparison['workloads'][name]['p95_change_percent'],abs_tol=1e-10) and change<=10
summary=[]
for group in cost['groups']:
 assert len(group['cases'])==group['matched']==30
 assert len(group['go']['records'])==len(group['rust']['records'])==30
 for case,g,r in zip(group['cases'],group['go']['records'],group['rust']['records']):
  assert g['id']==r['id']==case['id'] and g['status']==r['status']==200 and not g['error'] and not r['error']
  assert common.comparable(g)==common.comparable(r),(group['target_port'],case)
 for side in ['go','rust']:
  row=group[side];arrivals=row['response_arrival_ms'];intervals=row['response_interarrival_ms'];assert len(arrivals)==len(intervals)==30 and row['exit']==0
  recomputed=[arrivals[0]]+[b-a for a,b in zip(arrivals,arrivals[1:])]
  assert all(math.isclose(x,y,abs_tol=1e-9) for x,y in zip(intervals,recomputed)) and all(x>=0 for x in intervals)
  expected_connections=30 if group['target_port']=='080' and side=='rust' else 1
  assert row['accepted_connections']==expected_connections
  summary.append(dict(proxy_scheme=group['proxy_scheme'],target_port=group['target_port'],side=side,connections=row['accepted_connections'],later_median_ms=statistics.median(recomputed[1:])))
output=dict(source=source,all_samples=240,sets=sets,unchanged_comparator=comparison,all_cost_pairs=120,cost=summary,load_observations=len(load['observations']),size={side:report['binaries'][side]['identity']['size_bytes'] for side in ['go','rust']},historical_size_reference=18330466,independent=True)
Path('/tmp/symaira-fetch773-proxy-auth-independent-value.json').write_text(json.dumps(output,indent=2)+'\n');print(json.dumps(output,indent=2))
