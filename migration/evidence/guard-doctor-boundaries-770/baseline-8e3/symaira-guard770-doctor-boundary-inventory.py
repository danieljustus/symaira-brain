from pathlib import Path
import sys,json,tempfile,time,subprocess,hashlib,os
ROOT=Path('/workspace/symaira-guard770-doctor-boundaries')
sys.path.insert(0,str(ROOT/'scripts/guard-standalone-oracle'));import replay
GO=Path('/tmp/symaira-guard770-diagnostics-independent-go');RUST=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard')
configs={
'inline-defaults':b'defaults={read="allow"}\n','inline-sequence':b'sequence={enabled=true,threshold=3}\n','inline-proxy-audit':b'proxy={upstream="owned"}\naudit={path="owned",encrypt=true,encrypt_age="owned"}\n','empty-rules':b'rules=[]\n','inline-rules':b'rules=[{decision="allow",match={server="owned"}}]\n','empty-remote':b'remote=[]\n','inline-remote':b'remote=[{name="owned",provider="owned",host="owned",allowed_servers=["owned"],labels=["owned"]}]\n','inline-spawn-empty':b'spawn={allowlist=[]}\n','inline-invalid-decision':b'defaults={read="bad"}\n','inline-type-error':b'defaults={read=1}\n','inline-ordered-type-error':b'defaults={read="bad"}\nproxy={upstream=1}\n','unknown-scalar':b'unknown=1\n','unknown-nested':b'[proxy]\nunknown="owned"\n','unknown-ordered':b'unknown=1\n[unknown_table]\nb=1\na=2\n','decode-before-validation':b'[defaults]\nread="bad"\n[proxy]\nupstream=1\n','unknown-before-type-error':b'unknown=1\n[proxy]\nupstream=1\n','type-error-order-a':b'[proxy]\nupstream=1\n[audit]\nencrypt="x"\n','type-error-order-b':b'[audit]\nencrypt="x"\n[proxy]\nupstream=1\n','many-invalid-defaults':b'[defaults]\na="bad"\nb="worse"\n','valid-dotted-config':b'rules=[{decision="allow",match.server="owned"}]\n'}
anchors={
'known-surrogate':b'{"content_hash":"\\ud800"}', 'known-low-surrogate':b'{"last_entry_hash":"\\udc00"}', 'known-invalid-utf8':b'{"content_hash":"\xe2\x82"}', 'known-pair':b'{"content_hash":"\\ud800\\udc00"}', 'unknown-surrogate-key':b'{"\\ud800":1}', 'unknown-invalid-key':b'{"\xff":1}', 'known-string-before-error':b'{"content_hash":"\\ud800","entry_count":true}', 'known-error-before-key':b'{"entry_count":true,"\\ud800":1}', 'invalidutf8-after-error':b'{"entry_count":true,"content_hash":"\xff"}', 'known-type-invalid':b'{"entry_count":"\\ud800"}', 'unknown-valid-deep':b'{"unknown":"\\ud800","entry_count":1}', 'invalidutf8-outside-string':b'{"entry_count":\xff}', 'known-escaped-backslash':b'{"content_hash":"\\\\ud800"}'}
rows=[]
with tempfile.TemporaryDirectory(prefix='guard770-boundary-inventory-') as tmp:
 root=Path(tmp)
 selected=[('config',k,v) for k,v in configs.items()]+[('anchor',k,v) for k,v in anchors.items()]+[('filesystem',k,b'') for k in ['config-directory','anchor-directory','audit-parent-file','missing-command','malformed-discovery']]
 for i,(kind,name,data) in enumerate(selected):
  pair={}
  for side,binary,native in [('go',GO,False),('rust',RUST,True)]:
   owned=root/str(i)/side;env=replay.setup(owned,'empty')
   config=owned/'home/.config/symguard/config.toml';log=owned/'data/symguard/audit.log';anchor=Path(str(log)+'.anchor')
   if kind=='config':config.parent.mkdir();config.write_bytes(data)
   if kind=='anchor':log.parent.mkdir();log.write_bytes(b'{}\n');anchor.write_bytes(data)
   if name=='config-directory':config.mkdir(parents=True)
   if name=='anchor-directory':log.parent.mkdir();log.write_bytes(b'{}\n');anchor.mkdir()
   if name=='audit-parent-file':log.parent.write_bytes(b'owned')
   if name in ['missing-command','malformed-discovery']:
    path=owned/'home/.cursor/mcp.json';path.write_bytes(b'{"mcpServers":{"owned":{}}}' if name=='missing-command' else b'{bad')
   start=time.time();p=subprocess.run([str(binary),'doctor'],cwd=owned/'project',env=env,capture_output=True,timeout=5);end=time.time()
   pair[side]=dict(exit_code=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex(),normalized_stdout_hex=replay.normalized_stream(p.stdout,owned,['doctor'],native,True).hex(),normalized_stderr_hex=replay.normalized_stream(p.stderr,owned,['doctor'],native,False).hex(),files=replay.state_files(owned,start,end,False))
  rows.append(dict(kind=kind,id=name,input_hex=data.hex(),go_positive=pair['go']['exit_code']==0,native_gated=pair['rust']['stdout_hex']=='',matched=replay.comparable(pair['go'])==replay.comparable(pair['rust']),**pair))
 # Preserve actual nondeterminism, without imposing a new accepted contract.
 nondeterministic=[]
 for i in range(50):
  owned=root/'repeat'/str(i);env=replay.setup(owned,'empty');config=owned/'home/.config/symguard/config.toml';config.parent.mkdir();config.write_bytes(configs['many-invalid-defaults'])
  p=subprocess.run([str(GO),'doctor'],cwd=owned/'project',env=env,capture_output=True,timeout=5)
  nondeterministic.append(dict(exit=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex(),normalized_stdout_hex=replay.normalized_stream(p.stdout,owned,['doctor'],False,True).hex()))
report=dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),binaries={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (GO,RUST)},total=len(rows),results=rows,repeated_invalid_defaults=nondeterministic,distinct_invalid_default_reports=len(set(x['normalized_stdout_hex'] for x in nondeterministic)))
Path('/tmp/symaira-guard770-doctor-boundary-inventory.json').write_text(json.dumps(report,indent=2)+'\n')
for r in rows:print(r['id'],'positive',r['go_positive'],'gated',r['native_gated'],'match',r['matched'])
print('invalid defaults50runs distinct',report['distinct_invalid_default_reports'])
