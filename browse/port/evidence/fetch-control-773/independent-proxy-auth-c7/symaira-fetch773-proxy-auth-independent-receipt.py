from pathlib import Path
import os,json,hashlib,subprocess,re
repo=Path('/workspace/symaira-fetch773-proxy-auth');target=repo/'target';prefix='/tmp/symaira-fetch773-proxy-auth-independent-';sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip();assert head=='5b94a66159db34d335bc32b98bbcd2cb1dfbe5d9';assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
users=[]
for process in Path('/proc').iterdir():
 if not process.name.isdigit():continue
 for link in [process/'exe',process/'cwd',*list((process/'fd').glob('*'))]:
  try:value=os.readlink(link)
  except OSError:continue
  if value==str(target) or value.startswith(str(target)+'/'):users.append(dict(pid=process.name,link=str(link),value=value))
assert not users,users
load=lambda n:json.loads(Path(prefix+n+'.json').read_text())
p=load('process');raw=load('process-raw-proxy');auth=load('process-proxy-auth');boundary=load('boundaries');extra=load('auth-extra');required=load('auth-required');five=load('original5');eight=load('original8');value=load('value');provenance=load('provenance')
assert p['matched']==p['total']==251 and auth['echo_matched']==114 and auth['enforced_matched']==8 and raw['proxy_matched']==32 and raw['tls_matched']==4 and boundary['matched']==140
assert extra['total']==38 and extra['passed']==34 and required['total']==required['passed']==8 and five['passed']==5 and eight['passed']==7 and eight['failed']==1
rows=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',Path(prefix+'tests.log').read_text());assert sum(int(x[0]) for x in rows)==123 and sum(int(x[1]) for x in rows)==0 and sum(int(x[2]) for x in rows)==1 and len(rows)==26
files={str(p):dict(bytes=p.stat().st_size,sha256=sha(p)) for p in sorted(Path('/tmp').glob('symaira-fetch773-proxy-auth-independent-*')) if p.is_file() and p.name not in ['symaira-fetch773-proxy-auth-independent-review-receipt.json']}
result=dict(head=head,source='c7beb3a7a39dbc10f7e84769fccea5cf20e7f09e',main='e3dbda6cbb95429237d15a9b176209b7d107792c',worktree=str(repo),target=str(target),clean=True,disposition='APPROVED bounded correction; no actionable findings',findings=[],target_users=users,target_released=True,provenance=provenance,fresh=dict(original_process=251,original_controls=load('process-controls')['rejected'],raw_proxy=32,referer='7exact+1E011',linux_tls=4,native_jar=4,raw_controls=load('process-raw-proxy-controls')['rejected'],auth=114,enforcing=8,auth_controls=load('process-proxy-auth-controls')['rejected'],ordinary_passed=123,ordinary_failed=0,ordinary_ignored=1,summaries=26,independent_boundaries=140,all256_bytes_both_fields=True,original38='34equal4inherited origin mismatches',original_enforcing8='all200/200',original5='5exact',original8='7exact+1E011',strict='Clippy/fmt/actionlint PASS',go_packages=10,matrix='88contracts17acyclicworkitems',fixtures='28static11profiles15robots',benchmark_harness='7tests2platformskips',identity_tests=4),value=value,evidence=files,remaining=['Native six-platform CI before merge','Complete773/772/pipeline/default cutover acceptance','Inherited four origin nonUTF8 auth gaps','CONNECT/SOCKS/H2/trailer/framing contracts','macOS/Windows owned trusted CA success','Raw route connection reuse optimization'],commands='Actual Browse CWD runner, all-target/all-feature123tests+strict, pinned Go ten packages/fixtures, matrix/identity/benchmarkharness; exact logs retained',environment=dict(go='1.26.7',cargo_target=str(target),debug=0,testdebug=0,incremental=0,umask='022',subreaper='/tmp/symaira-subreaper.py'),authority='Independent review only; no source/GitHub edits, no new performance samples.')
Path(prefix+'review-receipt.json').write_text(json.dumps(result,indent=2)+'\n');print('Final receipt',head,'files',len(files),'target users0',sha(prefix+'review.md'),sha(prefix+'review-receipt.json'))
