import pathlib,importlib.util,tempfile,subprocess,json,hashlib,sys
root=pathlib.Path('/workspace/symaira-setup765-source');spec=importlib.util.spec_from_file_location('source',root/'scripts/setup-source-oracle/replay.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
go=pathlib.Path('/tmp/symaira-setup765-source-independent-go');rust=root/'target/debug/symbrain';original=m.configure;rows=[]
def configure(case,fixture,go,tool):
 env,args=original(case,fixture,go,tool);project=pathlib.Path(env['PROJECT']);p=project/'worker-tmp'
 if case['fault']=='file':p.write_text('owned relative obstruction')
 if case['fault']=='valid':p.mkdir()
 for key in ['TMPDIR','TMP','TEMP']:env[key]='worker-tmp'
 return env,args
m.configure=configure
with tempfile.TemporaryDirectory(prefix='source765-relative-tools-') as scratch:
 scratch=pathlib.Path(scratch);source=scratch/'fixture.go';source.write_bytes((root/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=scratch/'tool';subprocess.run(['go','build','-trimpath','-o',str(tool),str(source)],check=True)
 for fault in ['missing','file','valid']:
  case={'name':'relative-temp-'+fault,'fault':fault,'args':['setup','--from-source','<source>','--modules','browse','--json']};row={'case':case['name']}
  for label,binary in [('go',go),('rust',rust)]:
   with tempfile.TemporaryDirectory(prefix='source765-relative-case-') as fixture:row[label]=m.observe(binary,case,pathlib.Path(fixture),go,tool)
  row['mismatched_fields']=[k for k in row['go']['contract'] if row['go']['contract'][k]!=row['rust']['contract'][k]];rows.append(row)
 report={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'candidate_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=root)),'rust_binary_sha256':m.digest(rust),'go_binary_sha256':m.digest(go),'tool_binary_sha256':m.digest(tool),'observations':rows}
 pathlib.Path('/tmp/symaira-setup765-source-author-relative-temp.json').write_text(json.dumps(report,indent=2)+'\n');print([(r['case'],r['mismatched_fields']) for r in rows])
