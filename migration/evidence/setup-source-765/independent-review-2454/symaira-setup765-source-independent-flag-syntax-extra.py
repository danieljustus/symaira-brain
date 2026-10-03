import importlib.util,pathlib,json,subprocess,tempfile
root=pathlib.Path('/workspace/symaira-setup765-source');spec=importlib.util.spec_from_file_location('source_cfg',root/'scripts/setup-source-oracle/replay.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
go=pathlib.Path('/tmp/symaira-setup765-source-independent-go');rust=root/'target/debug/symbrain';results=[]
values=[
 ['setup','----from-source','<source>','--modules','browse','--json'],
 ['setup','--from-source','<source>','----modules','browse','--json'],
 ['setup','--from-source','<source>','--modules','browse','----json'],
 ['setup','--from-source','<source>','--=bad'],
 ['setup','--from-source','<source>','--modules','browse','---json'],
]
with tempfile.TemporaryDirectory(prefix='source765-config-tools-') as scratch:
 scratch=pathlib.Path(scratch);src=scratch/'fixture.go';src.write_bytes((root/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=scratch/'tool';subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True)
 for index,value in enumerate(values):
  case={'name':f'flag-syntax-extra-{index}','args':value};item={'case':case['name'],'argv':value}
  for label,binary in [('go',go),('rust',rust)]:
   with tempfile.TemporaryDirectory(prefix='source765-config-case-') as fixture:item[label]=m.observe(binary,case,pathlib.Path(fixture),go,tool)
  item['mismatched_fields']=[k for k in item['go']['contract'] if item['go']['contract'][k]!=item['rust']['contract'][k]];results.append(item)
 receipt={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'go_binary_sha256':m.digest(go),'rust_binary_sha256':m.digest(rust),'tool_binary_sha256':m.digest(tool),'observations':results};pathlib.Path('/tmp/symaira-setup765-source-independent-flag-syntax-extra.json').write_text(json.dumps(receipt,indent=2)+'\n');print([(r['case'],r['mismatched_fields']) for r in results])
