import pathlib,importlib.util,sys,os,subprocess,json,tempfile,base64,hashlib
ROOT=pathlib.Path('/workspace/symaira-doctor765-windows')
GO=pathlib.Path('/tmp/symaira-setup765-source-independent-go');RUST=pathlib.Path('/workspace/symaira-setup765-source/target/debug/symbrain')
def load(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);sys.modules[name]=m;spec.loader.exec_module(m);return m
m=load('sourceextra',ROOT/'scripts/setup-source-oracle/replay.py')
sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'));d=load('doctorextra',ROOT/'scripts/doctor-repair-oracle/replay.py')
source=[];doctor=[]
with tempfile.TemporaryDirectory(prefix='765-new-extra-tools-') as temp:
 temp=pathlib.Path(temp);src=temp/'fixture.go';src.write_bytes((ROOT/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=temp/'tool';subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True)
 tool_digest=m.digest(tool)
 for name,kw in [('raw-tmp-missing',{'raw_tmp':'missing'}),('raw-tmp-file',{'raw_tmp':'file'}),('raw-tmp-valid',{'raw_tmp':'valid'}),('raw-tmp-missing-payload',{'raw_tmp':'missing-payload'}),('raw-home-publication-file',{'raw_home_fault':True}),('raw-root-missing-browse',{'raw_root':os.fsdecode(b'owned\xff\xe2\x82'),'remove_browse':True})]:
  case=dict(m.cases()[0],name=name,**kw);item={'case':name}
  for label,binary in [('go',GO),('rust',RUST)]:
   with tempfile.TemporaryDirectory(prefix='765-new-source-case-') as root:item[label]=m.observe(binary,case,pathlib.Path(root),GO,tool)
  item['mismatched_fields']=[key for key in item['go']['contract'] if item['go']['contract'][key]!=item['rust']['contract'][key]];source.append(item)
 probe=d.build_probe(temp)
 with d.legacy.ReleaseFixtureServer() as url:
  d.legacy.RELEASE_BASE_URL=url
  for name,args in [('extra-dash-fix',('doctor','----fix')),('extra-dash-json',('doctor','--fix','----json')),('extra-dash-force',('doctor','--fix','----force-release')),('bad-syntax',('doctor','--fix','--=bad')),('valid-triple-force',('doctor','--fix','---force-release'))]:
   case=d.cases()[0].__class__(name,args=args,version=b'{"version":"0.0.0"}',provenance=b'{"source":"brain-source"}',verifier=True);item={'case':name,'args':args}
   for label,binary in [('go',GO),('rust',RUST)]:
    with tempfile.TemporaryDirectory(prefix='765-new-doctor-case-') as root:item[label]=d.observe(binary,case,pathlib.Path(root),probe,GO)
   item['mismatched_fields']=[key for key in item['go']['contract'] if item['go']['contract'][key]!=item['rust']['contract'][key]];doctor.append(item)
receipt={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'candidate_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),'source_head':'0b56d4edc7f9e6728fdbf955df89c06e2a1689d2','go_sha256':m.digest(GO),'rust_sha256':m.digest(RUST),'tool_sha256':tool_digest,'source_cases':source,'doctor_cases':doctor}
pathlib.Path('/tmp/symaira-doctor765-windows-0b56-findings.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Source:',[(x['case'],x['mismatched_fields']) for x in source]);print('Doctor:',[(x['case'],x['mismatched_fields']) for x in doctor])
