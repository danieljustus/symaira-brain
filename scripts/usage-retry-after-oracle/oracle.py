"""Actual frozen production callers, target SDK conversion and private TLS."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from peer import Peer

FROZEN='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
sha=lambda path:hashlib.sha256(path.read_bytes()).hexdigest()
def main():
 parser=argparse.ArgumentParser();parser.add_argument('output',type=Path);args=parser.parse_args()
 repo=Path(__file__).resolve().parents[2];output=args.output.resolve();output.parent.mkdir(parents=True,exist_ok=True);evidence=output.with_suffix('.evidence');evidence.mkdir(exist_ok=True)
 def run(label,command,env=None,cwd=None):
  p=subprocess.run(command,env=env,cwd=cwd or repo,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=300)
  (evidence/(label+'.log')).write_bytes(p.stdout)
  assert p.returncode==0,(label,p.returncode,p.stdout[-10000:])
 with tempfile.TemporaryDirectory(prefix='usage-retry-owned-')as temporary:
  root=Path(temporary);source=root/'go-source';peer=None
  run('input',['python3',str(repo/'scripts/usage-retry-after-oracle/cases.py'),str(evidence/'input.json')])
  corpus=json.loads((evidence/'input.json').read_text());assert len(corpus['public'])==1600 and len(corpus['wire'])==1164
  (evidence/'status-input.json').write_text(json.dumps(corpus['wire'],indent=2)+'\n')
  run('source',['git','worktree','add','--quiet','--detach',str(source),FROZEN])
  try:
   for label in ['scalar','status']:shutil.copyfile(repo/f'scripts/usage-retry-after-oracle/{label}_test.go.txt',source/f'internal/usage/retry_{label}_768_test.go')
   shutil.copyfile(repo/'scripts/usage-next-oracle/certificate.go.txt',root/'certificate.go')
   env=os.environ.copy();env.update(GOTOOLCHAIN='go1.26.7',CGO_ENABLED='0',USAGE_RETRY_INPUT=str(evidence/'input.json'),USAGE_RETRY_GO=str(evidence/'scalar-go.json'),USAGE_RETRY_NATIVE=str(evidence/'native.json'))
   run('certificate',['go','run',str(root/'certificate.go'),str(root)],env)
   for name in ['ca.pem','leaf.pem']:shutil.copyfile(root/name,evidence/name)
   suffix='.exe'if os.name=='nt'else'';go_binary=evidence/('go-provider-tests'+suffix)
   run('go-build',['go','-C',str(source),'test','-c','-trimpath','-o',str(go_binary),'./internal/usage'],env)
   run('go-scalars',[str(go_binary),'-test.run','^TestUsageRetryAfterScalar768$','-test.count=1'],env,source/'internal/usage')
   peer=Peer(root,corpus['wire'],b'{}',b'{}')
   peer.fixtures={row['fixture']:(source/'internal/usage/testdata'/row['fixture']).read_bytes()for row in corpus['wire']}
   env.update(USAGE_DEVICE_PEER=peer.address,USAGE_DEVICE_CA=str(root/'ca.pem'),USAGE_STATUS_ROOT=str(root/'homes'),USAGE_STATUS_INPUT=str(evidence/'status-input.json'),USAGE_STATUS_GO=str(evidence/'status-go.json'),USAGE_STATUS_NATIVE=str(evidence/'status-native.json'),USAGE_STATUS_HOSTS='|api.kimi.com||www.kimi.com||api.anthropic.com||chatgpt.com||api.github.com||cursor.com||api.moonshot.ai||portal.nousresearch.com||opencode.ai||openrouter.ai|')
   run('go-tls',[str(go_binary),'-test.run','^TestUsageRetryAfterTLS768$','-test.count=1'],env,source/'internal/usage')
   peer.phase='rust'
   run('native',['cargo','test','--locked','-p','symbrain-usage','--lib','retry_after_oracle_matches_fresh_go','--','--ignored','--nocapture'],env)
   matches=re.findall(r'Running unittests [^\n]*\(([^)]+)\)',(evidence/'native.log').read_text());assert len(matches)==1
   live=Path(matches[0]);live=live if live.is_absolute()else repo/live
   target=Path(env.get('CARGO_TARGET_DIR',repo/'target')).resolve();assert live.resolve().is_relative_to(target)
   native_binary=evidence/('native-provider-tests'+suffix);shutil.copy2(live,native_binary)
   main_wire=list(peer.rows);native=json.loads((evidence/'native.json').read_text());assert native['public_cases']==1600 and native['tls_cases']==native['statuses']['cases']==1164
   controls=[]
   for kind,field,message in [('missing-scalar','scalar-go.json','complete public scalar Go corpus'),('hex-rounding','scalar-go.json','public RetryAfter binary64'),('negative-status','status-go.json','actual remote full status/report'),('finite-only-waiver','scalar-go.json','public RetryAfter binary64')]:
    path=evidence/field;original=path.read_bytes();data=json.loads(original)
    if kind=='missing-scalar':data['records'].pop()
    elif kind=='hex-rounding':next(r for r in data['records']if r['id']=='grammar-8')['bits']='4010000000000001'
    elif kind=='finite-only-waiver':next(r for r in data['records']if r['id']=='grammar-12')['bits']=None
    else:data[3]['report']['providers'][0]['error']='owned incorrectly accepted negative retry'
    path.write_bytes(json.dumps(data).encode('utf-8'));peer.phase='control-'+kind
    try:
     command=[str(native_binary),'retry_after_oracle_matches_fresh_go','--ignored','--nocapture']
     p=subprocess.run(command,env=env,cwd=repo,capture_output=True,timeout=120);raw=p.stdout+p.stderr;(evidence/(kind+'.log')).write_bytes(raw)
     assert p.returncode==101 and message.encode()in raw and b'1 failed'in raw,(kind,p.returncode,raw[-10000:])
     controls.append(dict(id=kind,exit=p.returncode,intended_assertion=message,rejected=True,mutated_input_sha256=sha(path)))
    finally:path.write_bytes(original)
   (evidence/'controls.json').write_text(json.dumps(controls,indent=2)+'\n')
   peer.close()
   (evidence/'controls-wire.json').write_text(json.dumps(dict(rows=peer.rows[len(main_wire):],errors=peer.errors),indent=2)+'\n')
   assert not peer.errors,peer.errors
   peer=None
   (evidence/'wire.json').write_text(json.dumps(main_wire,indent=2)+'\n')
   observations=[]
   go=json.loads((evidence/'status-go.json').read_text());assert len(go)==1164
   for row in go:
    for index,request in enumerate(row['requests']):
     actual={phase:next(r for r in main_wire if r['phase']==phase and r['id']==row['id']and r['index']==index)for phase in ['go','rust']}
     for phase,data in actual.items():
      assert data['body_hex']==request['body_hex']
      for name,values in request['header_value_hex'].items():
       if name.lower()=='x-server-instance'and phase=='rust':assert data['headers_hex'][name.lower()]==['server-fn:00000000-0000-0000-0000-000000000000'.encode().hex()]
       else:assert data['headers_hex'][name.lower()]==values,(phase,row['id'],name)
     assert actual['go']['response_hex']==actual['rust']['response_hex']and actual['go']['request_line_hex']==actual['rust']['request_line_hex']
     observations.append(dict(id=row['id'],index=index,requestline_body_response_equal=True,provider_header_bytes_equal_except_inherited_opencode_instance=row['provider']=='opencode',provider_header_bytes_equal=row['provider']!='opencode',full_raw_request_equal=actual['go']['request_hex']==actual['rust']['request_hex']))
   assert len(main_wire)==2*len(observations)and len({(r['phase'],r['id'],r['index'])for r in main_wire})==len(main_wire)
   assert not subprocess.check_output(['git','-C',str(source),'diff','--name-only'])
   frozen={}
   for p in sorted((source/'internal/usage').rglob('*'))+[source/'go.mod',source/'go.sum']:
    if not p.is_file()or p.name in ['retry_scalar_768_test.go','retry_status_768_test.go']:continue
    name=str(p.relative_to(source));assert p.read_bytes()==subprocess.check_output(['git','-C',str(source),'show',FROZEN+':'+name]);frozen[name]=sha(p)
   head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip();dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=repo))
   paths=sorted((repo/'rust/symbrain-usage/src').rglob('*.rs'))+sorted((repo/'scripts/usage-retry-after-oracle').glob('*'))+[repo/name for name in ['Cargo.lock','rust/symbrain-usage/Cargo.toml','migration/licenses/go-strconv-bsd.txt','.github/workflows/ci.yml','migration/contract-matrix.csv','docs/adr/usage-retry-after-768.md','scripts/run-external-env.sh','scripts/usage-next-oracle/certificate.go.txt']];paths=[p for p in paths if p.is_file()]
   candidate={str(p.relative_to(repo)):sha(p)for p in paths}
   if not dirty:
    for p in paths:assert p.read_bytes()==subprocess.check_output(['git','show',head+':'+str(p.relative_to(repo))],cwd=repo)
   sdk=Path(subprocess.check_output(['go','env','GOROOT'],env=env,text=True).strip());sdk_paths=['src/internal/strconv/atof.go','src/internal/strconv/atoi.go','src/internal/strconv/deps.go','src/cmd/compile/internal/ssa/_gen/AMD64.rules','src/cmd/compile/internal/ssa/_gen/ARM64.rules','src/cmd/compile/internal/base/flag.go']
   receipt=dict(candidate_head=head,candidate_dirty=dirty,source_sha256=candidate,frozen_commit=FROZEN,frozen_source_sha256=frozen,go_sdk_source_sha256={p:sha(sdk/p)for p in sdk_paths},go_sdk=subprocess.check_output(['go','version'],env=env,text=True).strip(),go_environment=json.loads(subprocess.check_output(['go','env','-json','GOOS','GOARCH','GOEXPERIMENT','GOFLAGS'],env=env,text=True)),rust_sdk=subprocess.check_output(['rustc','-Vv'],env=env,text=True),scalar_cases=1600,tls_constructor_cases=1164,native=native,actual_target=dict(goos=native['goos'],goarch=native['goarch'],int_bits=native['int_bits'],scope='Actual current native SDK target conversion; no cross-target/runtime inference.'),wire_observations=observations,controls=controls,binaries={kind:dict(path=str(p),bytes=p.stat().st_size,sha256=sha(p))for kind,p in [('go',go_binary),('native',native_binary)]},evidence_sha256={p.name:sha(p)for p in evidence.iterdir()if p.is_file()},limits='Owned private CA/resolver keeps original public HTTPS hostnames verified; production builder/executor/constructor/status parser, full logical report/request/read-only metadata equality. Existing OpenCode runtime/fixed instance and transport default headers remain observed raw differences. Fast private TLS does not claim production deadline/cancel/proxy equivalence. No operator credentials/provider network; all original six Usage gates still required separately.')
   output.write_text(json.dumps(receipt,indent=2)+'\n');print('PASS1600 public scalars,1164 full TLS constructors;',len(observations),'wire pairs;4 actual controls')
  finally:
   if peer:
    (evidence/'wire-failed.json').write_text(json.dumps(dict(rows=peer.rows,errors=peer.errors),indent=2)+'\n');peer.close()
   subprocess.run(['git','-C',str(repo),'worktree','remove','--force',str(source)],check=True)
if __name__=='__main__':main()
