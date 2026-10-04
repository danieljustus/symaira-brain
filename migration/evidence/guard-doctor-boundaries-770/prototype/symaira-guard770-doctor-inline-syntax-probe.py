from pathlib import Path
import sys,tempfile,subprocess,json,time
ROOT=Path('/workspace/symaira-guard770-doctor-boundaries');sys.path.insert(0,str(ROOT/'scripts/guard-standalone-oracle'));import replay
GO=Path('/tmp/symaira-guard770-diagnostics-independent-go');RUST=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard')
inputs=[b'defaults={read="allow",}\n',b'defaults={\nread="allow"\n}\n',b'defaults={read="allow",\nnetwork="ask"}\n',b'defaults={read="allow",# hi\nnetwork="ask"}\n',b'proxy={upstream="\\e"}\n',b'proxy={upstream="\\x41"}\n',b'proxy={upstream="\\u0041"}\n',b'rules=[{decision="allow",match={command_contains=[\n"owned",\n]}}]\n',b'remote=[{labels=[\n"owned",\n]}]\n',b'defaults={read="allow"} #comment\n',b'rules=[{decision="allow",match={server="owned"}},]\n',b'proxy.upstream="\\e"\n']
rows=[]
with tempfile.TemporaryDirectory(prefix='guard770-inline-syntax-') as raw:
 for i,data in enumerate(inputs):
  pair={}
  for side,binary,native in [('go',GO,False),('rust',RUST,True)]:
   root=Path(raw)/str(i)/side;env=replay.setup(root,'empty');config=root/'home/.config/symguard/config.toml';config.parent.mkdir();config.write_bytes(data);start=time.time();p=subprocess.run([str(binary),'doctor'],cwd=root/'project',env=env,capture_output=True,timeout=5);end=time.time()
   pair[side]=dict(exit=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex(),normalized_stdout_hex=replay.normalized_stream(p.stdout,root,['doctor'],native,True).hex())
  rows.append(dict(input_hex=data.hex(),matched=pair['go']['exit']==pair['rust']['exit'] and pair['go']['normalized_stdout_hex']==pair['rust']['normalized_stdout_hex'],**pair));print(data,pair['go']['exit'],pair['rust']['exit'],rows[-1]['matched'])
Path('/tmp/symaira-guard770-doctor-inline-syntax-probe.json').write_text(json.dumps(dict(rows=rows),indent=2)+'\n')
