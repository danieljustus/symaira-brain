import base64, hashlib, importlib.util, json, os, pathlib, subprocess, sys, tempfile
ROOT=pathlib.Path('/workspace/symaira-doctor765-windows')
GO=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0')
RUST=pathlib.Path('/workspace/symaira-setup765-source/target/debug/symbrain')
def load(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);sys.modules[name]=m;spec.loader.exec_module(m);return m
s=load('source_review',ROOT/'scripts/setup-source-oracle/replay.py')
sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'))
d=load('doctor_review',ROOT/'scripts/doctor-repair-oracle/replay.py')
records=[]
with tempfile.TemporaryDirectory(prefix='doctor-source-independent-extra-') as temporary:
 tmp=pathlib.Path(temporary);src=tmp/'fixture.go';src.write_bytes((ROOT/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=tmp/'tool'
 subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True,env={**os.environ,'CGO_ENABLED':'0','GO111MODULE':'off'})
 for name,identity in [('unicode-space-valid',b'\xc2\xa0valid tool\xc2\xa0\n'),('unicode-space-malformed',b'\xc2\xa0invalid\xff\xe2\x82\xc2\xa0\n'),('ascii-space-malformed',b' invalid\xff\xe2\x82 \n')]:
  case=dict(s.cases()[0],name=name,env={'SOURCE_TOOL_VERSION_BYTES':base64.b64encode(identity).decode()});row={'case':name,'identity_bytes':base64.b64encode(identity).decode()}
  for label,binary in [('go',GO),('rust',RUST)]:
   with tempfile.TemporaryDirectory(prefix='source-extra-case-') as root:row[label]=s.observe(binary,case,pathlib.Path(root),GO,tool)
  row['differences']=[key for key in row['go']['contract'] if row['go']['contract'][key]!=row['rust']['contract'][key]];records.append(row)
 probe=d.build_probe(tmp);probe_sha=d.digest(probe)
 original=d.configure
 def raw_home(case,root,env,probe):
  env['HOME']=str(root/os.fsdecode(b'home\xff\xe2\x82'))
  original(case,root,env,probe)
 d.configure=raw_home
 with d.legacy.ReleaseFixtureServer() as url:
  d.legacy.RELEASE_BASE_URL=url
  for name,version,provenance in [('doctor-raw-home-correct',None,None),('doctor-raw-home-protected',b'{"version":"0.0.0"}',b'{"source":"brain-source"}'),('doctor-raw-home-directory',b'{"version":"0.0.0"}','directory')]:
   case=d.cases()[0].__class__(name,version=version,provenance=provenance);row={'case':name}
   for label,binary in [('go',GO),('rust',RUST)]:
    with tempfile.TemporaryDirectory(prefix='doctor-extra-case-') as root:row[label]=d.observe(binary,case,pathlib.Path(root),probe,GO)
   row['differences']=[key for key in row['go']['contract'] if row['go']['contract'][key]!=row['rust']['contract'][key]];records.append(row)
 receipt={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),'oracle':s.ORACLE,'go_sha':s.digest(GO),'rust_sha':s.digest(RUST),'tool_sha':s.digest(tool),'probe_sha':probe_sha,'cases':records}
 pathlib.Path('/tmp/symaira-doctor765-home-final-eight-extra.json').write_bytes((json.dumps(receipt,indent=2)+'\n').encode())
 print([(row['case'],row['differences']) for row in records])
