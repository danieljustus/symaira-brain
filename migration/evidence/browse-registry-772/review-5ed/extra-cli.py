import sys,os,pathlib,tempfile,json,subprocess,base64,importlib.util,hashlib
repo=pathlib.Path('/workspace/symaira-daemon772-registry');spec=importlib.util.spec_from_file_location('registry',repo/'browse/port/harness/daemon_registry.py');mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
records=[]
with tempfile.TemporaryDirectory(prefix='registry-review-extra-')as directory:
 root=pathlib.Path(directory);env=mod.environment(root);env['SYMBROWSE_NO_AUTOSTART']='1'
 names=[b'',b'bad\xffsession',b'bad\xe2\x82session',b'bad\xc0\xafsession',b'bad\xef\xbf\xbdsession',b'../escape',b'_invalid',b'a'*65,b'bad\nsession',b'bad\t"\\session']
 before=sorted(str(p.relative_to(root))for p in root.rglob('*'))
 for command in [('session','list'),('session','info'),('daemon','status'),('state','list')]:
  for raw in names:
   for fmt in [[],['--json'],['--output','yaml']]:
    arguments=[*map(os.fsencode,command),b'--session',raw,*map(os.fsencode,fmt)];pair={}
    for name,binary in [('go','/tmp/symaira-pr801-go-1.26.7'),('rust',str(repo/'target/debug/symbrowse'))]:
     p=subprocess.run([os.fsencode(binary),*arguments],env=env,cwd=root,capture_output=True,timeout=15);pair[name]={'exit':p.returncode,'stdout_base64':base64.b64encode(p.stdout).decode(),'stderr_base64':base64.b64encode(p.stderr).decode()}
    records.append({'arguments_base64':[base64.b64encode(x).decode()for x in arguments],'raw_session_hex':raw.hex(),'command':list(command),'format':fmt,'matches':pair['go']==pair['rust'],**pair})
 assert before==sorted(str(p.relative_to(root))for p in root.rglob('*')),'extra invalid CLI wrote state'
r={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),'go_sha256':hashlib.sha256(pathlib.Path('/tmp/symaira-pr801-go-1.26.7').read_bytes()).hexdigest(),'rust_sha256':hashlib.sha256((repo/'target/debug/symbrowse').read_bytes()).hexdigest(),'cases':len(records),'passed':sum(x['matches']for x in records),'failed':sum(not x['matches']for x in records),'no_files_created':True,'records':records};pathlib.Path('/tmp/symaira-registry-772-corrected-independent-extra-cli.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items()if k!='records'}));print('fail_groups',sorted(set((tuple(x['command']),x['raw_session_hex'])for x in records if not x['matches'])))
