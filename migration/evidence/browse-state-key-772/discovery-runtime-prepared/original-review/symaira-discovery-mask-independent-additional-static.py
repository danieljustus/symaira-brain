from pathlib import Path
import ast,hashlib,json,subprocess
root=Path('/workspace/symaira-daemon772-state-key-discovery-mask')
sha=lambda b:hashlib.sha256(b).hexdigest()
git=lambda *a:subprocess.check_output(['git',*a],cwd=root)
old=json.loads(Path('/tmp/symaira-discovery718-independent-provenance.json').read_bytes())
assert len(old['source_maps'])==177
for p,h in old['source_maps'].items():assert sha(git('show','381e324852e70fd7c647dce9bbffccb4c2731484:'+p))==h,p
new=json.loads((root/'migration/evidence/browse-state-key-772/discovery-mask-prepared/source_receipt.json').read_bytes())
for p,h in new['source_sha256'].items():assert sha((root/p).read_bytes())==h,p
sdk=Path('/workspace/toolchains/go1.26.7')
for p,h in new['sdk_sources'].items():assert sha((sdk/p).read_bytes())==h,p
path='browse/port/harness/daemon_provider_discovery.py'
a=ast.parse(git('show','381e3248:'+path).decode());b=ast.parse((root/path).read_text())
func=lambda t,n:next(x for x in t.body if isinstance(x,ast.FunctionDef) and x.name==n)
for name in ['build','stop_owned','install','pairs','script_vectors']:
 assert ast.dump(func(a,name),include_attributes=False)==ast.dump(func(b,name),include_attributes=False),name
for name,marker in [('observe','ledger'),('setting_vectors','rows')]:
 def tail(t):
  f=func(t,name)
  for parent in ast.walk(f):
   body=getattr(parent,'body',None)
   if not isinstance(body,list):continue
   for i,x in enumerate(body):
    if isinstance(x,ast.Assign) and any(isinstance(y,ast.Name) and y.id==marker for y in x.targets):
     return ast.dump(ast.Module(body=body[i:],type_ignores=[]),include_attributes=False)
  raise AssertionError((name,marker,'marker absent'))
 assert tail(a)==tail(b),name
changes=git('diff','--name-only','381e3248..7e5c4561').decode().splitlines()
allowed={'browse/crates/symbrowse-core/src/key_sources/startup_discovery/godebug.rs',path,'browse/port/harness/discovery_owner_control.go.in'}
assert all(p in allowed or p.startswith(('migration/evidence/','docs/adr/')) for p in changes)
for p in root.glob('browse/port/harness/*.py'):ast.parse(p.read_text(),str(p))
assert not git('status','--porcelain')
assert not (root/'target').exists() and not (root/'browse/target').exists()
out=dict(head=git('rev-parse','HEAD').decode().strip(),source='445c19688aa8f669ea99e15e6517005f6156b775',old177_original_git_bindings_verified=True,new8_source_maps_verified=True,new4_SDK_maps_verified=True,original_execution_assertion_cleanup_AST_unchanged=True,no_source_dependency_fixture_workflow_changes=True,clean=True,compiler_or_runtime=False)
Path('/tmp/symaira-discovery-mask-independent-additional-static.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out))
