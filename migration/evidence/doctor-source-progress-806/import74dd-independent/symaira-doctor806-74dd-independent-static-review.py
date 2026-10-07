from pathlib import Path
import ast,gzip,hashlib,json,subprocess,sys
root=Path('/workspace/symaira-doctor806-journal-import');head='74dd7aea5c7a1765dc36c2cbab2ca3fe6b6665d8';parent='8359c0bda1b7a0bfc14d2b8e69e1479fec9d182f'
def sha(raw):return hashlib.sha256(raw).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==head
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
path='scripts/setup-source-oracle/replay.py';old=subprocess.check_output(['git','show',parent+':'+path],cwd=root);new=(root/path).read_bytes();compile(new,path,'exec')
def funcs(raw):return {n.name:ast.dump(n,include_attributes=False) for n in ast.parse(raw).body if isinstance(n,(ast.FunctionDef,ast.AsyncFunctionDef,ast.ClassDef))}
assert funcs(old)==funcs(new)
loads=[]
for source in [Path('/workspace/symaira-doctor765')/path,root/path]:
 code='''import importlib.util,sys,types\nspec=importlib.util.spec_from_file_location("owned_source_review",sys.argv[1]);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);print(len(m.cases()),m.Journal.__module__)'''
 result=subprocess.run([sys.executable,'-I','-c',code,str(source)],cwd='/tmp',capture_output=True,timeout=5)
 loads.append({'path':str(source),'exit':result.returncode,'stdout_hex':result.stdout.hex(),'stderr_hex':result.stderr.hex()})
assert loads[0]['exit']==1 and b"No module named 'progress'" in bytes.fromhex(loads[0]['stderr_hex']);assert loads[1]['exit']==0 and bytes.fromhex(loads[1]['stdout_hex'])==b'111 setup_source_progress\n'
code='''import importlib.util,sys,types\nsys.modules["progress"]=types.ModuleType("unrelated_progress")\nspec=importlib.util.spec_from_file_location("owned_source_review",sys.argv[1]);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);assert m.Journal.__module__=="setup_source_progress";print("unrelated progress module not selected")'''
shadow=subprocess.run([sys.executable,'-I','-c',code,str(root/path)],cwd='/tmp',capture_output=True,timeout=5);assert shadow.returncode==0 and not shadow.stderr
folder=root/'migration/evidence/doctor-source-progress-806/import-boundary-8359';retention=json.loads((folder/'retention.json').read_bytes())
for row in retention:
 data=(root/row['archive']).read_bytes();raw=gzip.decompress(data);assert sha(data)==row['gzip_sha256'] and sha(raw)==row['sha256'] and len(raw)==row['bytes']
counts={}
for filename,expected in [('10-output-probes.json.gz',4),('12-source-output.json.gz',2),('08-pipe-probes.json.gz',6),('06-human-pipe-probes.json.gz',6),('04-embedded.json.gz',24)]:
 report=json.loads(gzip.decompress((folder/filename).read_bytes()));assert len(report['cases'])==expected
 assert all(row['differences']==[] for row in report['cases']);counts[filename]=expected
controls=json.loads(gzip.decompress((folder/'02-controls.json.gz').read_bytes()));assert controls['rejected_groups']==4
assert sum(r['exact_rejected_cases'] for r in controls['groups'])==15 and sum(r['preserved_matching_cases'] for r in controls['groups'])==3
review=json.loads(gzip.decompress((folder/'00-review.json.gz').read_bytes()))
for name,expected in review['binaries'].items():assert sha((Path('/tmp/symaira-doctor806-journal-import-root')/name).read_bytes())==expected
changed=subprocess.check_output(['git','diff','--name-only',parent,head],cwd=root,text=True).splitlines();assert len(changed)==22 and all(p==path or p.startswith(('docs/','migration/evidence/')) for p in changed)
receipt={'result':'APPROVE scoped source-only import correction','head':head,'parent':parent,'changed_files':changed,'production_rust_go_cargo_unchanged':True,'replay_function_ASTs_unchanged':len(funcs(old)),'replay_old_sha256':sha(old),'replay_new_sha256':sha(new),'independent_isolated_python_imports':loads,'shadow_progress_module_exit':shadow.returncode,'full_original_gzip_raw_maps_reverified':len(retention),'retained_actual_output_case_counts':counts,'retained_actual_control_groups':4,'rejected_cases':15,'preserved_partial_human_cases':3,'retained_actual_binaries_sha256_verified':review['binaries'],'no_new_compiler_cli_provider_target_port':True,'limitations':'Prior actual Linux reports bind unchanged archived exact58 binaries and prepared source module; no fresh Cargo/new-head product run. Windows25s cause/diagnostic mechanism still unproved. Approval is only for narrow sibling import binding, not product/native acceptance.'}
Path('/tmp/symaira-doctor806-74dd-independent-static-review.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({'head':head,'disposition':'APPROVE narrow import delta','functions':len(funcs(old)),'original19':len(retention),'cases':sum(counts.values()),'actualcontrols':4,'binarySHAverified':3,'new_product_runs':0}))
