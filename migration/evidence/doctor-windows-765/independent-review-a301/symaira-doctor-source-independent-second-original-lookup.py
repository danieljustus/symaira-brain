import pathlib,importlib.util,sys,os,subprocess,json,tempfile,shutil
ROOT=pathlib.Path('/workspace/symaira-doctor765-windows');spec=importlib.util.spec_from_file_location('lookup_extra',ROOT/'scripts/setup-source-oracle/replay.py');m=importlib.util.module_from_spec(spec);sys.modules[spec.name]=m;spec.loader.exec_module(m);GO=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0');RUST=pathlib.Path('/workspace/symaira-setup765-source/target/debug/symbrain');original=m.configure;rows=[]
def configure(case,root,go,tool):
 env,args=original(case,root,go,tool)
 for name in ('git','go'):shutil.copyfile(tool,pathlib.Path(env['PROJECT'])/name);(pathlib.Path(env['PROJECT'])/name).chmod(0o755)
 if case['path_mode']=='unset':env.pop('PATH',None)
 else:env['PATH']=''
 if case.get('optin'):env['GODEBUG']='execerrdot=0'
 return env,args
m.configure=configure
with tempfile.TemporaryDirectory(prefix='765-independent-lookup-tools-') as tmp:
 tmp=pathlib.Path(tmp);src=tmp/'fixture.go';src.write_bytes((ROOT/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=tmp/'tool';subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True);toolsha=m.digest(tool)
 for name,mode,opt in [('empty-path','empty',False),('unset-path','unset',False),('empty-path-optin','empty',True)]:
  case=dict(m.cases()[0],name=name,path_mode=mode,optin=opt);row={'case':name}
  for label,binary in [('go',GO),('rust',RUST)]:
   with tempfile.TemporaryDirectory(prefix='765-independent-lookup-case-') as root:row[label]=m.observe(binary,case,pathlib.Path(root),GO,tool)
  row['mismatched_fields']=[k for k in row['go']['contract'] if row['go']['contract'][k]!=row['rust']['contract'][k]];rows.append(row)
r={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'source_head':'0b56d4edc7f9e6728fdbf955df89c06e2a1689d2','rust_sha256':m.digest(RUST),'go_sha256':m.digest(GO),'tool_sha256':toolsha,'observations':rows};pathlib.Path('/tmp/symaira-doctor-source-independent-second-original-lookup.json').write_text(json.dumps(r,indent=2)+'\n');print([(x['case'],x['mismatched_fields']) for x in rows])
