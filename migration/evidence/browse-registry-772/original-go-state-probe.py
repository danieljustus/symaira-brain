import importlib.util,json,os,tempfile
from pathlib import Path
spec=importlib.util.spec_from_file_location('registry','/workspace/symaira-daemon772-registry/browse/port/harness/daemon_registry.py'); module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
source=Path('/workspace/oracles/daemon772-go-source'); api=module.oracle_api(source);fixtures=api['observations']['state_fixtures_hex'];records=[]
for binary in [Path('/tmp/symaira-pr801-go-1.26.7'),Path('/workspace/symaira-daemon772/target/debug/symbrowse')]:
 with tempfile.TemporaryDirectory(prefix='br-original-') as directory:
  root=Path(directory);env=module.environment(root);session='original'+str(os.getpid());child,endpoint=module.start(binary,root,env,session)
  try:
   before=module.harness.request(endpoint,{'cmd':'state.list','session':session})
   states=Path(env['XDG_STATE_HOME'])/'symbrowse/states';states.mkdir(parents=True,exist_ok=True)
   for name,hexbytes in fixtures.items():states.joinpath(name+'.json').write_bytes(bytes.fromhex(hexbytes))
   shown=module.harness.request(endpoint,{'cmd':'state.show','session':session,'args':{'name':'alpha'}})
   after=states.joinpath('alpha.json').read_bytes().hex()
   records.append({'binary':str(binary),'sha256':module.process.digest(binary),'empty_list':before,'show_go_null_cookies':shown,'retained_file_unchanged':after==fixtures['alpha']})
  finally:module.stop(child,endpoint,session)
report={'original_rust_head':'a49b25285e8b241f8b0636c8d2b5da164bd63850','original_rust_production_head':'6c3bf1f93d2760b17f4f936076d26f0f4c00fe1d','go_ref':module.process.GO_REF,'oracle_api':api,'observations':records,'probe_sha256':module.process.digest(Path(__file__))}
Path('/tmp/symaira-registry-original-state.json').write_text(json.dumps(report,indent=2)+'\n')
print([(r['empty_list'],r['show_go_null_cookies']) for r in records])
