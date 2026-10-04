import pathlib,tempfile,subprocess,os,stat,hashlib,json,base64,resource,signal
ROOT=pathlib.Path('/workspace/symaira-policy-create-race');BIN=ROOT/'target/debug/symbrain';results=[]
def snapshot(root):
 out={}
 for p in sorted(root.rglob('*')):
  st=p.lstat();row={'mode':stat.S_IMODE(st.st_mode)}
  if p.is_symlink():row.update(type='symlink',target=os.readlink(p))
  elif p.is_dir():row.update(type='directory')
  else:row.update(type='file',sha256=hashlib.sha256(p.read_bytes()).hexdigest(),size=st.st_size)
  out[str(p.relative_to(root))]=row
 return out
for name in ('dangling-final-symlink','final-symlink-other-file','final-directory','parent-is-file','traversal','write-failure'):
 with tempfile.TemporaryDirectory(prefix='policy-create-independent-') as t:
  root=pathlib.Path(t);cfg=root/'config/symbrain';cfg.mkdir(parents=True);profiles=cfg/'profiles';outside=root/'unrelated';outside.write_bytes(b'protected unrelated bytes')
  env={'HOME':str(root/'home'),'XDG_CONFIG_HOME':str(root/'config'),'XDG_DATA_HOME':str(root/'data'),'XDG_CACHE_HOME':str(root/'cache'),'PATH':str(root/'absent-tools'),'SYMBRAIN_GO_BINARY':str(root/'absent-go')}
  if name=='parent-is-file':profiles.write_bytes(b'protected parent obstruction')
  else:profiles.mkdir()
  target=profiles/'race.toml';arg='race';preexec=None
  if name=='dangling-final-symlink':target.symlink_to(root/'absent-target')
  elif name=='final-symlink-other-file':target.symlink_to(outside)
  elif name=='final-directory':target.mkdir()
  elif name=='traversal':arg='../unrelated'
  elif name=='write-failure':
   def limit():
    signal.signal(signal.SIGXFSZ,signal.SIG_IGN);resource.setrlimit(resource.RLIMIT_FSIZE,(8,8))
   preexec=limit
  before=snapshot(root);p=subprocess.run([str(BIN),'profile','add',arg,'--from','restricted'],cwd=root,env=env,capture_output=True,preexec_fn=preexec,timeout=5);after=snapshot(root)
  assert p.returncode!=0,(name,p.stdout,p.stderr)
  assert before==after,(name,before,after)
  if name=='write-failure':assert b'File too large' in p.stderr,p.stderr
  results.append({'case':name,'exit':p.returncode,'stdout_base64':base64.b64encode(p.stdout).decode(),'stderr_base64':base64.b64encode(p.stderr).decode(),'before':before,'after':after,'no_mutation':True,'write_failure_limit':8 if name=='write-failure' else None})
receipt={'source_head':'0e71ef1450dc3b9fde52ce9b2dac8d77aa96eb98','publication_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'native_cli_sha256':hashlib.sha256(BIN.read_bytes()).hexdigest(),'cases':results,'scope':'Linux actual CLI with absent Go fallback; SIGXFSZ explicitly ignored only in owned child so injected write failure returns I/O error and exercises ordinary RAII cleanup, not abrupt termination cleanup'}
pathlib.Path('/tmp/symaira-policy-create-race-independent-extra.json').write_text(json.dumps(receipt,indent=2)+'\n');print([(x['case'],x['exit'],x['no_mutation']) for x in results])
