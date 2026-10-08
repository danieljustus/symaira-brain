import base64, hashlib, json, os, pathlib, subprocess, tempfile
PREFIX='/tmp/symaira-usage768-argv-final2'
GO='/tmp/symaira-usage768-f759-root-owner-go'
RUST=PREFIX+'-public-probe'
results=[]
def digestfiles(root):
    return {os.fsencode(p).hex():hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file() and not p.is_symlink()}
for provider in ['copilot','kimi']:
  for variant in ['raw-home','raw-symlink-parent','relative-home','relative-symlink-parent','empty-home','unset-home','leading-parent','repeated-slash','file-parent-erased','missing-parent-erased','unicode-home','explicit-raw','explicit-raw-symlink','explicit-legacy','legacy-symlink-parent','env-preempt-invalid-file']:
    with tempfile.TemporaryDirectory(prefix='usage768-review-owned-') as directory:
      root=pathlib.Path(directory)
      rawhome=b'home-\xff\xe2\x82'
      for name in [b'home',rawhome,b'owner/nested',b'owner/home',b'plain',b'home-unicode-\xcf\x80\xe9\x9b\xaa']:
        os.makedirs(os.fsencode(root)+b'/'+name,exist_ok=True)
      (root/'link').symlink_to(root/'owner/nested',target_is_directory=True)
      (root/'file-parent').write_bytes(b'owned regular obstruction')
      relative='.config/github-copilot/apps.json' if provider=='copilot' else '.kimi-code/credentials/kimi-code.json'
      for owner,name in [('lexical',b'home'),('raw',rawhome),('physical',b'owner/home'),('unicode',b'home-unicode-\xcf\x80\xe9\x9b\xaa'),('cwd',b'')]:
        path=os.fsencode(root)+b'/'+name+b'/'+relative.encode();os.makedirs(os.path.dirname(path),exist_ok=True)
        value={'github.com:a':{'oauth_token':'env://'+owner.upper()}} if provider=='copilot' else {'access_token':'env://'+owner.upper()}
        with open(path,'wb') as f:f.write(json.dumps(value).encode())
      home=os.fsencode(root)+b'/home';overrides={}
      if variant=='raw-home':home=os.fsencode(root)+b'/'+rawhome
      elif variant=='raw-symlink-parent':home=os.fsencode(root)+b'/link/../'+rawhome
      elif variant=='relative-home':home=b'home'
      elif variant=='relative-symlink-parent':home=b'link/../home'
      elif variant=='empty-home':home=b''
      elif variant=='unset-home':home=None
      elif variant=='leading-parent':home=b'plain/../../'+root.name.encode()+b'/home'
      elif variant=='repeated-slash':home=os.fsencode(root)+b'//home///./'
      elif variant=='file-parent-erased':home=os.fsencode(root)+b'/file-parent/../home'
      elif variant=='missing-parent-erased':home=os.fsencode(root)+b'/missing-parent/../home'
      elif variant=='unicode-home':home=os.fsencode(root)+b'/home-unicode-\xcf\x80\xe9\x9b\xaa'
      elif variant=='explicit-raw':overrides[b'KIMI_CODE_HOME']=os.fsencode(root)+b'/'+rawhome+b'/.kimi-code'
      elif variant=='explicit-raw-symlink':overrides[b'KIMI_CODE_HOME']=os.fsencode(root)+b'/link/../'+rawhome+b'/.kimi-code'
      elif variant in ['explicit-legacy','legacy-symlink-parent'] and provider=='kimi':
        (root/'home/.kimi-code/credentials/kimi-code.json').unlink()
        p=root/'home/.kimi/credentials/kimi-code.json';p.parent.mkdir(parents=True);p.write_bytes(b'{"access_token":"env://LEGACY"}')
        if variant=='explicit-legacy':overrides[b'KIMI_CODE_HOME']=os.fsencode(root)+b'/file-parent/../home/.kimi'
        else:home=os.fsencode(root)+b'/link/../home'
      elif variant=='env-preempt-invalid-file':
        if provider=='copilot':overrides[b'COPILOT_ACCESS_TOKEN']=b'env://ABSENT'
        else:overrides[b'KIMI_CODE_API_KEY']=b'env://ABSENT'
        (root/'home'/relative).write_bytes(b'{"oauth_token":false,"access_token":false}')
      env={b'PATH':b'',b'ANTHROPIC_OAUTH_TOKEN':b'env://ABSENT',b'SYMBRAIN_GO_BINARY':os.fsencode(root)+b'/absent-go'}
      if home is not None:env[b'HOME']=home;env[b'USERPROFILE']=home
      env.update(overrides)
      before=digestfiles(root);link=os.fsencode(os.readlink(root/'link'));observations=[]
      for binary in [GO,RUST]:
        r=subprocess.run([binary,provider],env=env,cwd=root,capture_output=True,timeout=15)
        observations.append(dict(exit=r.returncode,stdout_b64=base64.b64encode(r.stdout).decode(),stderr_b64=base64.b64encode(r.stderr).decode(),parsed=json.loads(r.stdout) if r.returncode==0 else None))
      actual=observations[1]['parsed'].copy();gate=actual.pop('needs_go');expected=observations[0]['parsed']
      matches=expected==actual and not gate and all(x['exit']==0 and x['stderr_b64']=='' for x in observations)
      after=digestfiles(root);assert before==after and os.fsencode(os.readlink(root/'link'))==link
      results.append(dict(provider=provider,variant=variant,home_hex=None if home is None else home.hex(),env_hex={k.hex():v.hex() for k,v in env.items()},before=before,after=after,link_hex=link.hex(),go=observations[0],native=observations[1],needs_go=gate,matches=matches,read_only=True))
receipt=dict(cases=len(results),passed=sum(x['matches'] for x in results),failed=sum(not x['matches'] for x in results),records=results,binary_sha256={p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest() for p in [GO,RUST]},runner_sha256=hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),scope='actual frozen Go public constructors and native production rlib, configuration/source/status/Authorization bytes; owned canned401; Unix raw bytes; not native Windows proof')
assert receipt['cases']==receipt['passed']==32 and receipt['failed']==0
pathlib.Path(PREFIX+'-extra.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(receipt['cases'],receipt['passed'],receipt['failed'])
for x in results:
  if not x['matches']:print(x['provider'],x['variant'],x['go']['parsed'],x['native']['parsed'])
