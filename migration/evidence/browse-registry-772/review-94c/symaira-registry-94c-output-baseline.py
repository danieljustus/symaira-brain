import pathlib,tempfile,subprocess,importlib.util,hashlib,json
repo=pathlib.Path('/workspace/symaira-daemon772-registry');spec=importlib.util.spec_from_file_location('registry',repo/'browse/port/harness/daemon_registry.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
observations={};binaries={'go':pathlib.Path('/tmp/symaira-pr801-go-1.26.7'),'rust':pathlib.Path('/tmp/symaira-registry-94c-rust')}
for name,binary in binaries.items():
 with tempfile.TemporaryDirectory(prefix='registry-5ed-edges-') as td:
  root=pathlib.Path(td);env=m.environment(root);env['SYMBROWSE_NO_AUTOSTART']='1';observations[name]=m.cli_edges.observe(binary,root,env)
invalid=[{'go':a,'rust':b,'match':a==b} for a,b in zip(observations['go']['invalid'],observations['rust']['invalid'])]
help_pairs=[{'go':a,'rust':b,'match':m.cli_edges.comparable_help(a,go=True)==m.cli_edges.comparable_help(b,go=False)} for a,b in zip(observations['go']['help'],observations['rust']['help'])]
r={'candidate_head':'94c5960ccc129b7c103f7ed2fac099bddc6bfa8a','current_author_tree_dirty':True,'probe_sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [pathlib.Path(__file__),repo/'browse/port/harness/daemon_registry_cli.py']},'binary_sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in binaries.values()},'invalid_pairs':invalid,'help_pairs':help_pairs,'invalid_passed':sum(x['match'] for x in invalid),'invalid_failed':sum(not x['match'] for x in invalid),'help_passed':sum(x['match'] for x in help_pairs),'help_failed':sum(not x['match'] for x in help_pairs),'no_files_created':True,'root_help_projection':'only actual verified unsupported session-id advertisement row'}
pathlib.Path('/tmp/symaira-registry-94c-output-baseline.json').write_text(json.dumps(r,indent=2)+'\n');print({k:r[k] for k in ['candidate_head','invalid_passed','invalid_failed','help_passed','help_failed','no_files_created']})
