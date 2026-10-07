import pathlib,tempfile,subprocess,json,base64,importlib.util,hashlib
repo=pathlib.Path('/workspace/symaira-daemon772-registry');s=importlib.util.spec_from_file_location('registry',repo/'browse/port/harness/daemon_registry.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m);records=[]
with tempfile.TemporaryDirectory(prefix='registry-review-help-')as directory:
 root=pathlib.Path(directory);env=m.environment(root);env['SYMBROWSE_NO_AUTOSTART']='1';before=sorted(str(p.relative_to(root))for p in root.rglob('*'))
 for args in [['session','--help'],['session'],['session','list','--help'],['session','info','--help'],['help','session','list'],['help','session','info']]:
  pair={}
  for name,binary in [('go','/tmp/symaira-pr801-go-1.26.7'),('rust',str(repo/'target/debug/symbrowse'))]:
   p=subprocess.run([binary,*args],env=env,cwd=root,capture_output=True,timeout=10);pair[name]={'exit':p.returncode,'stdout_base64':base64.b64encode(p.stdout).decode(),'stderr_base64':base64.b64encode(p.stderr).decode()}
  records.append({'arguments':args,'matches':pair['go']==pair['rust'],**pair})
 assert before==sorted(str(p.relative_to(root))for p in root.rglob('*'))
r={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),'go_sha256':hashlib.sha256(pathlib.Path('/tmp/symaira-pr801-go-1.26.7').read_bytes()).hexdigest(),'rust_sha256':hashlib.sha256((repo/'target/debug/symbrowse').read_bytes()).hexdigest(),'cases':6,'passed':sum(x['matches']for x in records),'failed':sum(not x['matches']for x in records),'no_files_created':True,'records':records};pathlib.Path('/tmp/symaira-registry-772-corrected-independent-help.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items()if k!='records'}))
for row in records:
 print('arguments',row['arguments'],'match',row['matches'],'first_go_line',base64.b64decode(row['go']['stdout_base64']).decode().splitlines()[0],'first_rust_line',base64.b64decode(row['rust']['stdout_base64']).decode().splitlines()[0])
