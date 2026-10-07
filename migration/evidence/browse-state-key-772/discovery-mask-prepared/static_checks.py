"""Read-only checks for this mask successor; no compiler/provider invocation."""
from pathlib import Path
import ast,builtins,gzip,hashlib,json,subprocess,symtable
root=Path(__file__).resolve().parents[4]
base='381e324852e70fd7c647dce9bbffccb4c2731484'
sha=lambda raw:hashlib.sha256(raw).hexdigest()
folder=root/'migration/evidence/browse-state-key-772'
retained={}
for name,key in [('discovery-request-718','files'),('independent-ccc','raw_proofs'),('discovery-request-48fc','proofs')]:
 r=json.loads((folder/name/'receipt.json').read_bytes())
 for row in r[key]:
  compressed=(folder/name/row['retained']).read_bytes();raw=gzip.decompress(compressed)
  assert sha(compressed)==row['gzip_sha256'] and sha(raw)==row['sha256'] and len(raw)==row['bytes']
 retained[name]=len(r[key])
r=json.loads((folder/'discovery-mask-prepared/actual-sdk-only/retention.json').read_bytes())
for row in r['files']:
 compressed=(folder/'discovery-mask-prepared/actual-sdk-only'/row['retained']).read_bytes();raw=gzip.decompress(compressed)
 assert sha(compressed)==row['gzip_sha256'] and sha(raw)==row['sha256'] and len(raw)==row['bytes']
retained['actual_sdk_only']=len(r['files'])
old=Path('/workspace/symaira-daemon772-state-key-discovery-clean')
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=old,text=True).strip()==base
assert not subprocess.check_output(['git','status','--porcelain'],cwd=old)
source_receipt=json.loads((old/'migration/evidence/browse-state-key-772/discovery-prepared-718/receipt.json').read_bytes())
for path,expected in source_receipt['candidate_source_sha256'].items():
 assert sha(subprocess.check_output(['git','show',base+':'+path],cwd=root))==expected
changes=subprocess.check_output(['git','diff','--name-only',base],cwd=root,text=True).splitlines()
allowed={'browse/crates/symbrowse-core/src/key_sources/startup_discovery/godebug.rs','browse/port/harness/daemon_provider_discovery.py','browse/port/harness/discovery_owner_control.go.in'}
assert all(path in allowed or path.startswith(('migration/evidence/','docs/adr/')) for path in changes)
standalone='browse/crates/symbrowse-core/src/key_sources/command/standalone.rs'
assert sha((root/standalone).read_bytes())=='28ace6936b05bf1d347feb1b442a94964663539bbfd5ce3738a45ac96d6af312'
harness=root/'browse/port/harness/daemon_provider_discovery.py';text=harness.read_text();table=symtable.symtable(text,str(harness),'exec')
defined={s.get_name() for s in table.get_symbols() if s.is_assigned() or s.is_imported()};used=set()
def inspect(table):
 for symbol in table.get_symbols():
  if symbol.is_referenced() and symbol.is_global():used.add(symbol.get_name())
 for child in table.get_children():inspect(child)
inspect(table)
assert not used-defined-set(dir(builtins))-{'__file__'}
tree=ast.parse(text)
supported=[];cases=[];unresolved=[]
for node in ast.walk(tree):
 if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='supported' for t in node.targets): supported+=ast.literal_eval(node.value)
 if isinstance(node,ast.AugAssign) and isinstance(node.target,ast.Name) and node.target.id=='supported':supported+=ast.literal_eval(node.value)
 if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='unresolved' for t in node.targets):unresolved=ast.literal_eval(node.value)
assert len(supported)==30 and len(unresolved)==8
spec=__import__('importlib.util',fromlist=['util']);loader=spec.spec_from_file_location('prepared_model',Path(__file__).parent/'projection.py');model=spec.module_from_spec(loader);loader.loader.exec_module(model)
for pattern,expected in supported:assert model.sdk_result(pattern)['allow_relative']==model.native_result(pattern)['allow_relative']==expected
for path in list((root/'browse/port/harness').glob('*.py'))+list(Path(__file__).parent.glob('*.py')):ast.parse(path.read_text(),str(path))
checks=[['/home/agent/.cargo/bin/rustfmt','--edition','2024','--check','browse/crates/symbrowse-core/src/key_sources/startup_discovery/godebug.rs'],['/workspace/toolchains/bin/actionlint','.github/workflows/browse-daemon-native.yml'],['git','diff','--check']]
for command in checks:
 result=subprocess.run(command,cwd=root,capture_output=True);assert result.returncode==0,(command,result.stderr)
for path in (root/'browse/port/harness').glob('discovery_*.go.in'):
 result=subprocess.run(['/workspace/toolchains/go1.26.7/bin/gofmt','-l',str(path)],capture_output=True);assert result.returncode==0 and not result.stdout
for path in [(root/'target'),(root/'browse/target')]:assert not path.exists()
sdk=Path('/workspace/toolchains/go1.26.7')
paths=list(allowed)+['docs/adr/daemon-provider-discovery-and-owner-772.md','migration/evidence/browse-state-key-772/discovery-mask-prepared/projection.py','migration/evidence/browse-state-key-772/discovery-mask-prepared/sdk_bulk.go.in','migration/evidence/browse-state-key-772/discovery-mask-prepared/sdk_only.py','migration/evidence/browse-state-key-772/discovery-mask-prepared/static_checks.py']
receipt=dict(base=base,original718_source_maps=len(source_receipt['candidate_source_sha256']),retention_maps=retained,source_sha256={p:sha((root/p).read_bytes()) for p in paths},sdk_sources={p:sha((sdk/p).read_bytes()) for p in ['src/internal/bisect/bisect.go','src/internal/godebug/godebug.go','LICENSE','bin/go']},prepared={'unix_cli':40,'windows_cli':49,'typed_supported':len(supported),'typed_unported':len(unresolved),'retained_optional':5,'windows_script':6,'unix_mutants':4,'windows_mutants':3,'new_units':2},checks='PASS rustfmt/actionlint/Python/global-scope/allGoFixtureFormatting/diff and immutable preservation',candidate_rust_builds=0,candidate_runtime=0,provider_runtime=0,ports=0,actual_sdk_runtime='separately bound actual-sdk-only/receipt; never candidate proof',native_acceptance=False)
(Path(__file__).parent/'source_receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({'original_source_maps':receipt['original718_source_maps'],'retention_maps':retained,'prepared':receipt['prepared'],'runtime':'SDK-only retained; candidate unbuilt'}))
