from pathlib import Path
import sys,os,json,tempfile,time,subprocess,hashlib
sys.path.insert(0,'/workspace/symaira-guard770-doctor-warnings/scripts/guard-standalone-oracle');import replay
GO=Path('/tmp/symaira-guard770-warnings-final-process-binaries/go');RUST=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard');rows=[]
with tempfile.TemporaryDirectory(prefix='guard770-independent-raw-') as tmp:
 root=Path(tmp)
 for i,kind in enumerate(('config-invalid-default','config-missing-equals','anchor-overflow','audit-directory')):
  args=['decide'] if kind=='audit-directory' else ['doctor'];payload=b'{"command":"owned","risk_class":"low"}' if args[0]=='decide' else b'';pair={}
  for side,binary,native in [('go',GO,False),('rust',RUST,True)]:
   owned=root/str(i)/side;env=replay.setup(owned,'empty');rawpart=os.fsdecode(b'owned-\xe2\x82')
   if kind.startswith('config'):
    path=owned/rawpart/'config.toml';path.parent.mkdir();path.write_bytes(b'[defaults]\nread="bad"\n' if kind=='config-invalid-default' else b'invalid TOML\n');env['SYMGUARD_CONFIG']=str(path)
   else:
    data=owned/rawpart;env['XDG_DATA_HOME']=str(data);log=data/'symguard/audit.log';log.parent.mkdir(parents=True)
    if kind=='audit-directory':log.mkdir()
    else:log.write_bytes(b'{}\n');Path(str(log)+'.anchor').write_bytes(b'{"entry_count":9223372036854775808}')
   start=time.time();p=subprocess.run([str(binary),*args],cwd=owned/'project',env=env,input=payload,capture_output=True,timeout=5);end=time.time()
   pair[side]=dict(exit_code=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex(),normalized_stdout_hex=replay.normalized_stream(p.stdout,owned,args,native,True).hex(),normalized_stderr_hex=replay.normalized_stream(p.stderr,owned,args,native,False).hex(),files=replay.state_files(owned,start,end,args[0]=='decide'))
  rows.append(dict(kind=kind,raw_component_hex=b'owned-\xe2\x82'.hex(),matched=replay.comparable(pair['go'])==replay.comparable(pair['rust']),**pair))
report=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd='/workspace/symaira-guard770-doctor-warnings',text=True).strip(),results=rows,binaries={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (GO,RUST)})
Path('/tmp/symaira-guard770-warnings-closure-raw-paths.json').write_text(json.dumps(report,indent=2)+'\n')
for r in rows:
 print(r['kind'],r['matched'])
 for s in ('go','rust'):print(s,r[s]['exit_code'],bytes.fromhex(r[s]['normalized_stdout_hex']))
