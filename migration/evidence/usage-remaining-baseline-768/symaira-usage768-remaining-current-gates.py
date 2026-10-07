from pathlib import Path
import base64,hashlib,json,os,subprocess,tempfile
native=Path('/workspace/oracles/symaira-usage768-argv-880-binaries/candidate-usage');public=Path('/tmp/symaira-usage768-argv-final2-public-probe');go=Path('/workspace/oracles/symaira-usage768-argv-880-binaries/go-usage')
assert hashlib.sha256(native.read_bytes()).hexdigest()=='f657a231ac1bae1472718e10c9ae30418d7589286b2300f882eb4b51b7b04743'
rows=[];vectors=[]
def add(name,provider,files=None,env=None,kind='regular'):
 vectors.append(dict(id=name,provider=provider,files=files or {},env=env or {},kind=kind))
k='.kimi-code/credentials/kimi-code.json';device='.kimi-code/device_id';c='.config/github-copilot/apps.json';claude='.claude/.credentials.json';nous='.hermes/auth.json'
add('empty-portable','kimi')
for value in ['设备','owned-café','\u00a0设备\u3000','\u0085设备\u2028','owned\u0085inside','owned\u2028inside','owned\u200binside','owned_ascii','\u3000owned_ascii\u3000','\u00a0\u3000']:
 add('device-'+value.encode().hex(),'kimi',{k:b'{"access_token":"owned-cli"}',device:value.encode()})
for name,value in [('ff',b'\xff'),('e282',b'\xe2\x82'),('lf',b'owned\ndevice'),('tab',b'owned\tdevice'),('del',b'owned\x7fdevice')]:
 add('device-control-'+name,'kimi',{k:b'{"access_token":"owned-cli"}',device:value})
for kind in ['symlink','directory','fifo']:
 add('device-source-'+kind,'kimi',{k:b'{"access_token":"owned-cli"}'},kind=kind)
add('device-irrelevant-api','kimi',{k:b'{}',device:'设备'.encode()},{'KIMI_CODE_API_KEY':'owned-api'})
add('kimi-token-control','kimi',{k:b'{"access_token":"owned\\ntoken"}'})
for kind in ['symlink','directory','fifo']:
 add('kimi-source-'+kind,'kimi',kind='credential-'+kind)
ambiguous=b'{"oauthAccount":{"a":{"accessToken":"owned-a"},"b":{"accessToken":"owned-b"}}}'
add('claude-distinct-nondefault','claude',{claude:ambiguous},{'ANTHROPIC_OAUTH_TOKEN':''})
add('claude-distinct-but-env-wins','claude',{claude:ambiguous},{'ANTHROPIC_OAUTH_TOKEN':'owned-env'})
add('claude-default-preempts-distinct','claude',{claude:b'{"oauthAccount":{"default":{"accessToken":"owned-default"},"a":{"accessToken":"a"},"b":{"accessToken":"b"}}}'},{'ANTHROPIC_OAUTH_TOKEN':''})
add('claude-identical-nondefault','claude',{claude:b'{"oauthAccount":{"a":{"accessToken":"owned"},"b":{"accessToken":"owned"}}}'},{'ANTHROPIC_OAUTH_TOKEN':''})
add('copilot-distinct-prefix','copilot',{c:b'{"github.com:a":{"oauth_token":"a"},"github.com:b":{"oauth_token":"b"}}'})
add('copilot-distinct-but-env-wins','copilot',{c:b'{"github.com:a":{"oauth_token":"a"},"github.com:b":{"oauth_token":"b"}}'},{'COPILOT_ACCESS_TOKEN':'owned-env'})
add('copilot-control-token','copilot',{c:b'{"github.com:a":{"oauth_token":"owned\\ntoken"}}'})
for kind in ['symlink','directory','fifo']:
 add('copilot-source-'+kind,'copilot',kind='copilot-'+kind)
payload=base64.urlsafe_b64encode(b'{"exp":1e30}').rstrip(b'=').decode();jwt='e30.'+payload+'.c2ln';value=json.dumps({'providers':[{'id':'nous','invoke_jwt':jwt}]}).encode()
add('nous-jwt-out-of-range','nous',{nous:value})
add('nous-jwt-but-env-wins','nous',{nous:value},{'NOUS_PORTAL_ACCESS_TOKEN':'owned-env'})
for provider,name in [('kimi','KIMI_CODE_BASE_URL'),('nous','HERMES_PORTAL_BASE_URL'),('openrouter','OPENROUTER_API_URL')]:
 for suffix,value in [('http','http://example.com'),('explicit-port','https://example.com:443'),('upper-host','https://EXAMPLE.com'),('percent-path','https://example.com/a%20b'),('simple-accepted','https://example.com/a_b')]:
  add(name+'-'+suffix,provider,env={name:value})
for workspace in ['wrk_good123','https://opencode.ai/workspace/wrk_good123',' wrk_good123 ','wrk_a-b']:
 add('workspace-cookie-'+workspace,'opencode',env={'OPENCODE_COOKIE':'owned-cookie','OPENCODE_WORKSPACE_ID':workspace})
 add('workspace-no-cookie-'+workspace,'opencode',env={'OPENCODE_WORKSPACE_ID':workspace})
with tempfile.TemporaryDirectory(prefix='usage768-remaining-current-owned-')as temporary:
 for index,v in enumerate(vectors):
  home=Path(temporary)/str(index);home.mkdir();env={'HOME':str(home),'USERPROFILE':str(home),'PATH':'','ANTHROPIC_OAUTH_TOKEN':'env://ABSENT','SYMBRAIN_GO_BINARY':str(home/'absent-go'),'XDG_CONFIG_HOME':str(home/'config'),'XDG_DATA_HOME':str(home/'data'),'XDG_CACHE_HOME':str(home/'cache')};env.update(v['env'])
  for name,value in v['files'].items():
   p=home/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(value)
  special=None
  if v['kind']!='regular':
   target=device
   if v['kind'].startswith('credential-'):target=k
   if v['kind'].startswith('copilot-'):target=c
   special=home/target;special.parent.mkdir(parents=True,exist_ok=True)
   if v['kind'].endswith('symlink'):
    peer=home/'owned-peer';peer.write_bytes(b'{"access_token":"owned-cli","github.com:a":{"oauth_token":"owned"}}');special.symlink_to(peer)
   elif v['kind'].endswith('directory'):special.mkdir()
   else:os.mkfifo(special)
  def state():
   result={}
   for p in home.rglob('*'):
    if p.is_symlink():result[str(p.relative_to(home))]=dict(kind='link',target=str(p.readlink()))
    elif p.is_file():result[str(p.relative_to(home))]=dict(kind='file',sha256=hashlib.sha256(p.read_bytes()).hexdigest())
    elif p.is_dir():result[str(p.relative_to(home))]=dict(kind='dir')
    else:result[str(p.relative_to(home))]=dict(kind='other',mode=p.lstat().st_mode)
   return result
  before=state();r=subprocess.run([str(public),v['provider']],cwd=home,env=env,capture_output=True,timeout=10);assert r.returncode==0 and not r.stderr,(v,r.returncode,r.stderr);selection=json.loads(r.stdout)
  observations=[]
  for binary in [go,native]:
   p=subprocess.run([str(binary),'usage','--owned-remaining-probe'],cwd=home,env=env,capture_output=True,timeout=10);observations.append(dict(exit=p.returncode,stdout_b64=base64.b64encode(p.stdout).decode(),stderr_b64=base64.b64encode(p.stderr).decode()))
  assert observations[0]['exit']==2 and not observations[0]['stdout_b64']
  assert (observations[1]['exit']==1)==selection['needs_go'],(v,selection,observations)
  assert before==state()
  rows.append(dict(id=v['id'],provider=v['provider'],kind=v['kind'],file_hex={p:data.hex()for p,data in v['files'].items()},env=v['env'],current=selection,go_cli=observations[0],current_cli=observations[1],read_only=True,state=before))
r=dict(source='8803847278546f3161eaaa84d2e65bc2eec594a2',head='eea73b7478e62f1bb174d456bbbecccc1de94034',cases=len(rows),requires_go=sum(x['current']['needs_go']for x in rows),native=sum(not x['current']['needs_go']for x in rows),records=rows,binary_sha256={str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in [native,go,public]},scope='Copied immutable actual CLI/public constructor probe; native public configured/status/source/Authorization with canned401 only. Actual Go/native invalidargv reveals current admission before parsing; full native200/wire parity not claimed. Owned regular/symlink/directory/FIFO inputs stay unchanged, native rejects FIFO nonblocking; no Go credential FIFO read, operator credentials or paid/public provider network. Linux only; Windows-home/macKeychain static inventory only.')
Path('/tmp/symaira-usage768-remaining-current-gates.json').write_text(json.dumps(r,indent=2)+'\n');print('PASS',len(rows),'owned copied-executable gates;',r['requires_go'],'Go-required,',r['native'],'native; no target build or mutation')
