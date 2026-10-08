import subprocess,json,hashlib,re
from pathlib import Path
R=Path('/workspace/symaira-fetch773');E=R/'browse/port/evidence/fetch-control-773/review-fixes-1f82';S='1f82e9cf8812c5b383a19349241787215a857493';H='52236234a197f00de89edbb185fc762a4f121161';G='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def git(*a):return subprocess.check_output(['git',*a],cwd=R)
summary=json.loads((E/'linux-source-bound-summary.json').read_text());rb=json.loads((E/'rust-build.json').read_text());gb=json.loads((E/'go-build.json').read_text());fresh=json.loads(Path('/tmp/symaira-fetch773-second-fix-independent-cwd-process.json').read_text())
checks={}
for label,manifest,revision in [('fetch30',summary['candidate_source_sha256'],S),('harness10',summary['harness_workflow_sha256'],S),('rust148',rb['source_sha256'],S),('go415',gb['source_sha256'],G)]:
 for name,digest in manifest.items():
  assert hashlib.sha256(git('show',revision+':'+name)).hexdigest()==digest,(label,name)
  if revision==S: assert sha(R/name)==digest,(label,name,'workingtree')
 checks[label]=len(manifest)
assert fresh['candidate_source_sha256']==summary['candidate_source_sha256'];assert fresh['go_source_sha256']==gb['source_sha256']
for name,digest in summary['evidence_file_sha256'].items():assert sha(E/name)==digest,name
logs=json.loads((E/'linux-exact-logs.json').read_text())
for name,v in logs.items():assert hashlib.sha256(v['utf8'].encode()).hexdigest()==v['sha256'],name
checks['evidence_digests']=len(summary['evidence_file_sha256']);checks['embedded_log_digests']=len(logs)
changed=git('diff','--name-only',S,H).decode().splitlines();assert len(changed)==14 and all(n.startswith('browse/port/evidence/fetch-control-773/review-fixes-1f82/') for n in changed)
assert not git('status','--porcelain');assert git('rev-parse','HEAD').decode().strip()==H
checks['evidence_only_files']=14
binaries={}
for label,p,digest in [('native_probe',R/'target/debug/examples/control_probe',fresh['binaries_sha256']['rust']),('fresh_go_probe',Path('/tmp/symaira-fetch773-second-fix-independent-go-probe'),fresh['binaries_sha256']['go']),('native_release',R/'target/release/symbrowse',rb['binary_sha256']),('go_release',Path('/tmp/symaira-fetch773-second-fix-go-release'),gb['binary_sha256'])]:
 assert sha(p)==digest,label;binaries[label]=dict(path=str(p),sha256=digest,size=p.stat().st_size)
for name,original in [('review.md','/tmp/symaira-fetch773-fixed-independent-review.md'),('receipt.json','/tmp/symaira-fetch773-fixed-independent-review-receipt.json')]:
 p=R/'browse/port/evidence/fetch-control-773/independent-review-a073'/name;assert p.read_bytes()==Path(original).read_bytes(),name
checks['original_a073_review_receipt_preserved']=True
bench={}
for name in ['linux-quiet','original-loaded']:
 p=E/(name+'-paired-30.json');out=subprocess.run(['python3',str(R/'browse/port/bench/compare.py'),str(R/'browse/docs/rust-port/baseline.json'),str(p)],capture_output=True);result=json.loads(out.stdout);stored=json.loads((E/(name+'-comparison.json')).read_text());assert result==stored
 report=json.loads(p.read_text());metrics={}
 for workload in ['cli','mcp','daemon','fetch']:
  metrics[workload]={}
  for impl in ['go','rust']:
   x=report['binaries'][impl][workload];assert len(x['raw_samples'])==30;assert x['status']=='pass';p95=sorted(s['duration_ns'] for s in x['raw_samples'])[28];assert p95==x['p95_duration_ns'];metrics[workload][impl]=p95
 bench[name]=dict(comparator_exit=out.returncode,result=result,samples=240,p95_ns=metrics)
 assert all(report['binaries'][i]['identity']['sha256']==binaries[('go' if i=='go' else 'native')+'_release']['sha256'] for i in ['go','rust'])
text=Path('/tmp/symaira-fetch773-second-fix-independent-tests.log').read_text();matches=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',text)
ordinary=dict(summaries=len(matches),passed=sum(int(x[0]) for x in matches),failed=sum(int(x[1]) for x in matches),ignored=sum(int(x[2]) for x in matches),compilations=text.count('Compiling '),isolated_children=text.count('test cache_environment_isolated_child ... ok'))
result=dict(candidate_head=H,source_revision=S,go_revision=G,source_clean=True,checks=checks,binaries=binaries,benchmark=bench,ordinary_tests=ordinary,go_sdk_client_sha256=sha(Path('/workspace/toolchains/go1.26.7/src/net/http/client.go')))
Path('/tmp/symaira-fetch773-second-fix-independent-provenance.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
