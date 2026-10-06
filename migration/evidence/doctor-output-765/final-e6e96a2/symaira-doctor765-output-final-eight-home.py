import importlib.util,pathlib,sys,tempfile,json,subprocess
ROOT=pathlib.Path('/workspace/symaira-doctor765-output');sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'))
spec=importlib.util.spec_from_file_location('home_doctor',ROOT/'scripts/doctor-repair-oracle/replay.py');d=importlib.util.module_from_spec(spec);sys.modules[spec.name]=d;spec.loader.exec_module(d)
GO=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0');RUST=pathlib.Path('/workspace/symaira-setup765-source/target/debug/symbrain');rows=[]
original=d.configure
with tempfile.TemporaryDirectory(prefix='doctor-home-probe-') as temporary,d.legacy.ReleaseFixtureServer() as url:
 tmp=pathlib.Path(temporary);probe=d.build_probe(tmp);d.legacy.RELEASE_BASE_URL=url
 for mode in ('empty','unset'):
  def configure(case,root,env,probe):
   original(case,root,env,probe)
   if mode=='unset':env.pop('HOME',None)
   else:env['HOME']=''
  d.configure=configure
  row={'case':'doctor-home-'+mode};case=d.cases()[0].__class__(row['case'],all_missing=True)
  for label,binary in [('go',GO),('rust',RUST)]:
   with tempfile.TemporaryDirectory(prefix='doctor-home-case-') as root:row[label]=d.observe(binary,case,pathlib.Path(root),probe,GO)
  row['differences']=[key for key in row['go']['contract'] if row['go']['contract'][key]!=row['rust']['contract'][key]];rows.append(row)
 r={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),'go_sha':d.digest(GO),'rust_sha':d.digest(RUST),'probe_sha':d.digest(probe),'cases':rows}
 pathlib.Path('/tmp/symaira-doctor765-output-final-eight-home.json').write_bytes((json.dumps(r,indent=2)+'\n').encode());print([(r['case'],r['differences']) for r in rows])
