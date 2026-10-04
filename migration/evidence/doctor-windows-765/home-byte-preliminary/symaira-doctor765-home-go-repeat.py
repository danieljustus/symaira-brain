import importlib.util,pathlib,sys,tempfile,json,subprocess,base64
ROOT=pathlib.Path('/workspace/symaira-doctor765-windows');sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'))
spec=importlib.util.spec_from_file_location('doctor_repeat',ROOT/'scripts/doctor-repair-oracle/replay.py');d=importlib.util.module_from_spec(spec);sys.modules[spec.name]=d;spec.loader.exec_module(d)
GO=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0');rows=[]
with tempfile.TemporaryDirectory(prefix='765-home-repeat-') as temporary,d.legacy.ReleaseFixtureServer() as url:
 tmp=pathlib.Path(temporary);probe=d.build_probe(tmp);d.legacy.RELEASE_BASE_URL=url
 case=next(c for c in d.cases() if c.name=='raw-home-force-directory')
 for index in range(2):
  with tempfile.TemporaryDirectory(prefix='765-go-repeat-case-') as root:rows.append(d.observe(GO,case,pathlib.Path(root),probe,GO))
 receipt={'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),'go_sha256':d.digest(GO),'probe_sha256':d.digest(probe),'observations':rows}
 pathlib.Path('/tmp/symaira-doctor765-home-go-repeat.json').write_text(json.dumps(receipt,indent=2)+'\n')
 for r in rows:
  print([base64.b64decode(line).decode('ascii') for line in r['contract']['logs']['cores']['symdesk'] if base64.b64decode(line).startswith(b'ERROR')])
