from pathlib import Path
import sys,json,subprocess,tempfile,hashlib,time,os
sys.path.insert(0,'/workspace/symaira-guard770-raw-paths/scripts/guard-standalone-oracle')
import replay
GO=Path('/tmp/symaira-guard770-diagnostics-independent-go');RUST=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard')
anchors=[b'{"entry_count":9223372036854775808,"content_hash":"\\ud800"}',b'{"content_hash":false,"entry_count":-9223372036854775809}',b'{"ENTRY_COUNT":null,"entry_count":1.5}',b'{"entry_count":false,"ENTRY_COUNT":null}',b'{"entry_count":-0,"LOG_SIZE":-9223372036854775808,"SCHEMA_VERSION":9223372036854775807}',b'{"schema_version":9223372036854775808}',b'{"entry_count":1E+999}',b'{"last_entry_hash":5,"content_hash":true}',b'{"entry_count":"oops","log_size":{}}',b'{"unknown":'+b'['*500+b'1e9999'+b']'*500+b',"entry_count":1}',b'{"\\u0065ntry_count":"wrong"}',b'{"content_ha\\u017fh":false}',b'{"entry_count":{},"content_hash":"ok"}',b'{"unknown":"\\ud800","log_size":0}',b'{"log_size":null,"content_hash":null}',b'{"last_entry_hash":[],"entry_count":false}']
configs=[('[defaults]\nfirst="bad"\n[[rules]]\ndecision="bad"\n','parity'),('[[rules]]\ndecision="allow"\nmatch.server="x"\n[[rules]]\ndecision="bad"\n[sequence]\nenabled=true\nthreshold=1\n','parity'),('[[rules]]\ndecision="allow"\n[sequence]\nenabled=true\nthreshold=1\n','parity'),('[sequence]\nenabled=true\nthreshold=1\n[[spawn.allowlist]]\npath="relative"\n','parity'),('[sequence]\nenabled=true\nthreshold=0\n[[spawn.allowlist]]\npath="relative"\n','parity'),('[[rules]]\ndecision="allow"\nmatch.command_contains=[""]\n','parity'),('[defaults]\nfirst="bad"\n[remote]\nname=123\n','gated'),('[defaults]\nfirst="bad"\n[proxy]\nupstream=123\n','gated'),('[defaults]\nfirst="bad"\n[audit]\nencrypt=true\n','parity'),('[defaults]\nfirst="bad"\n[audit]\nencrypt_age=[]\n','gated'),('[[rules]]\ndecision="bad"\nmatch.command_contains=[1]\n','gated'),('[sequence]\nenabled=false\nthreshold=1\n','parity'),('[defaults]\n"weird\\u0085key"="bad\\u007f"\n','parity'),('[[spawn.allowlist]]\npath="/owned/../missing"\nargv_prefix=[]\n','parity'),('[defaults]\nfirst="bad"\n[[remote]]\nname="x"\nprovider="x"\nhost="x"\ntrust_level="x"\nlabels=["x"]\nallowed_servers=[]\n','parity')]
rows=[]
with tempfile.TemporaryDirectory(prefix='guard770-independent-extra-') as tmp:
 root=Path(tmp)
 for i,(kind,data,contract) in enumerate([('anchor',v,'parity') for v in anchors]+[('config',v.encode(),c) for v,c in configs]):
  case=dict(id=f'{kind}-{i}',args=['doctor'],state='empty',payload=b'',contract='parity' if contract=='parity' else 'native-fail-closed')
  pair={}
  for side,binary,native in [('go',GO,False),('rust',RUST,True)]:
   owned=root/str(i)/side;env=replay.setup(owned,'empty')
   path=owned/'data/symguard/audit.log.anchor' if kind=='anchor' else owned/'home/.config/symguard/config.toml';path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
   if kind=='anchor':(owned/'data/symguard/audit.log').write_bytes(b'{}\n')
   start=time.time();p=subprocess.run([str(binary),'doctor'],cwd=owned/'project',env=env,capture_output=True,timeout=5);end=time.time()
   pair[side]=dict(exit_code=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex(),normalized_stdout_hex=replay.normalized_stream(p.stdout,owned,['doctor'],native,True).hex(),normalized_stderr_hex=replay.normalized_stream(p.stderr,owned,['doctor'],native,False).hex(),files=replay.state_files(owned,start,end,False))
  try:disposition=replay.compare(case,pair['go'],pair['rust'])
  except AssertionError as e:disposition='failed: '+str(e)
  case['payload_hex']=case.pop('payload').hex()
  rows.append(dict(case=case,kind=kind,input_hex=data.hex(),disposition=disposition,**pair))
report=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd='/workspace/symaira-guard770-raw-paths',text=True).strip(),total=len(rows),matched=sum(x['disposition']=='matched' for x in rows),gated=sum(x['disposition'].startswith('native-fail') for x in rows),failed=sum(x['disposition'].startswith('failed') for x in rows),results=rows,binaries={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (GO,RUST)})
Path('/tmp/symaira-guard806-root-extra.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k!='results'},indent=2));print('failures',[(x['case']['id'],x['input_hex']) for x in rows if x['disposition'].startswith('failed')])
