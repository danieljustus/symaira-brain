from pathlib import Path
import sys,json,subprocess,tempfile,hashlib,time
ROOT=Path('/workspace/symaira-guard770-doctor-warnings');sys.path.insert(0,str(ROOT/'scripts/guard-standalone-oracle'));import replay
GO=Path('/tmp/symaira-guard770-warnings-final-process-binaries/go');RUST=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard')
configs=[
 'defaults={read="allow",write="ask",network="deny"}\nrules=[]\nremote=[]\n',
 'proxy={upstream="a<>&\\u2028"}\naudit={path="a<>&\\u2029",encrypt=false}\n',
 'sequence={enabled=false,threshold=-9223372036854775808}\n',
 'sequence={enabled=true,threshold=9223372036854775807}\n',
 'rules=[{decision="readonly",match={capability="owned",command_contains=["a<>&","b"]}}]\n',
 'rules=[{decision="sandbox",match.tool="owned"},{decision="redact",match={server="owned"}}]\n',
 'remote=[{name="a",provider="owned",host="owned",trust_level="owned",labels=[],allowed_servers=[]},{}]\n',
 'defaults={}\nrules=[]\nproxy={}\naudit={}\nremote=[]\nsequence={}\nspawn={allowlist=[]}\n',
 'rules=[{decision="bad",match={server="owned"}},{decision="bad2",match={tool="owned"}}]\n',
 'rules=[{match={server="owned"}}]\n',
 'defaults={read="bad"}\nsequence={enabled=true,threshold=1}\n',
 'spawn={allowlist=[{path="relative",argv_prefix=[]}]}\n',
]
anchors=[
 b'{"content_hash":"\\udbff\\udfff","last_entry_hash":"\\ud800A\\udc00"}',
 b'{"content_hash":"\\ud800\\udbff\\udfff","log_size":-0}',
 b'{"content_hash":"\\udc00\\ud800","entry_count":null}',
 b'{"CONTENT_HASH":"\\ud800","content_hash":null}',
 b'{"content_hash":"\xff\xf0\x9f\x92","log_size":0}',
 b'{"content_hash":"\xed\xa0\x80\xe2\x82","log_size":0}',
 b'{"content_hash":"\\\\ud800","entry_count":0}',
 b'{"\\ud800":{"nested":"\\udc00","n":1e9999},"entry_count":0}',
 b'{"\\ufffd":1e9999,"content_hash":"\\ud800"}',
 b'{"content_hash":"\\ud800","entry_count":1e-9999}',
 b'{"entry_count":1e-9999,"content_hash":"\\udc00"}',
 b'{"content_hash":"\\ud800","schema_version":9223372036854775808}',
 b'{"entry_count":false,"content_hash":"\xff"}',
 b'{"log_size":[],"content_hash":"\\ud800"}',
 b'{"content_hash":"\\ud800","content_hash":4}',
 b'{"content_hash":4,"content_hash":"\\ud800"}',
 b'{"entry_count":"\\ud800","entry_count":1}',
 b'{"entry_count":{},"unknown":"\\ud800"}',
 b'{"unknown":[{"nested":"\xff"},{"n":1e9999}],"content_hash":"\\udc00"}',
 b'{"content_hash":"\\ud800","unknown":'+b'['*200+b'"\\udc00"'+b']'*200+b'}',
]
rows=[]
with tempfile.TemporaryDirectory(prefix='guard9603-extra-') as directory:
 root=Path(directory)
 for i,(kind,data) in enumerate([('config',x.encode()) for x in configs]+[('anchor',x) for x in anchors]):
  case=dict(id=f'independent-{kind}-{i}',args=['doctor'],state='empty',payload=b'',contract='parity');pair={}
  for side,binary,native in [('go',GO,False),('rust',RUST,True)]:
   owned=root/str(i)/side;env=replay.setup(owned,'empty');path=owned/'home/.config/symguard/config.toml' if kind=='config' else owned/'data/symguard/audit.log.anchor';path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
   if kind=='anchor':(owned/'data/symguard/audit.log').write_bytes(b'{}\n')
   began=time.time();p=subprocess.run([str(binary),'doctor'],cwd=owned/'project',env=env,capture_output=True,timeout=5);end=time.time();pair[side]=dict(exit_code=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex(),normalized_stdout_hex=replay.normalized_stream(p.stdout,owned,['doctor'],native,True).hex(),normalized_stderr_hex=replay.normalized_stream(p.stderr,owned,['doctor'],native,False).hex(),files=replay.state_files(owned,began,end,False))
  try:disposition=replay.compare(case,pair['go'],pair['rust'])
  except AssertionError as e:disposition='failed: '+str(e)
  case['payload_hex']=case.pop('payload').hex();rows.append(dict(case=case,input_hex=data.hex(),disposition=disposition,**pair))
r={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'source':'707e60997ae93ffb346e5a80e9bb135517fa6c15','original_corpus_source':'9603d24e6749ee67e5eac1b7ddda6900af022cd5','total':len(rows),'matched':sum(x['disposition']=='matched' for x in rows),'results':rows,'binaries':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [GO,RUST]}}
Path('/tmp/symaira-guard770-warnings-parent-new32.json').write_text(json.dumps(r,indent=2)+'\n');print('independent',r['matched'],'/',r['total']);print('failures',[(x['case']['id'],x['disposition']) for x in rows if x['disposition']!='matched']);assert r['matched']==r['total']
