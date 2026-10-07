import base64,hashlib,json,os,pathlib,subprocess,tempfile
prefix='/tmp/symaira-usage768-abf-root-doctor'
go=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0')
native=pathlib.Path('/workspace/symaira-usage768-local-files/target/debug/symbrain')
parent=pathlib.Path('/tmp/symaira-usage768-f759-root-parent-cli')
assert parent.is_file()
flags=[b'--help',b'---help',b'----help',b'----json',b'----not-a-usage-flag',b'--owned-\xff',b'--owned-\xe2\x82',b'owned-\xff',b'--not-a-usage-flag=\xff',b'--not-a-usage-flag',b'ordinary-argument']
rows=[]
for provider in ['copilot','kimi']:
 with tempfile.TemporaryDirectory(prefix='usage768-cli-review-')as directory:
  home=pathlib.Path(directory)
  file=home/('.config/github-copilot/apps.json'if provider=='copilot'else'.kimi-code/credentials/kimi-code.json')
  file.parent.mkdir(parents=True)
  data=b'{"github.com:a":{"oauth_token":"env://OWNED_LITERAL"}}'if provider=='copilot'else b'{"access_token":"env://OWNED_LITERAL"}'
  file.write_bytes(data)
  env={b'HOME':os.fsencode(home),b'USERPROFILE':os.fsencode(home),b'PATH':b'',b'ANTHROPIC_OAUTH_TOKEN':b'env://ABSENT',b'SYMBRAIN_GO_BINARY':os.fsencode(go)}
  for flag in flags:
   observations=[]
   for binary in [go,parent,native]:
    r=subprocess.run([os.fsencode(binary),b'usage',flag],env=env,capture_output=True,timeout=15)
    observations.append(dict(exit=r.returncode,stdout_b64=base64.b64encode(r.stdout).decode(),stderr_b64=base64.b64encode(r.stderr).decode()))
   assert file.read_bytes()==data
   rows.append(dict(provider=provider,arg_hex=flag.hex(),go=observations[0],parent=observations[1],native=observations[2],parent_matches=observations[0]==observations[1],native_matches=observations[0]==observations[2],file_sha256=hashlib.sha256(data).hexdigest(),read_only=True))
receipt=dict(cases=len(rows),passed=sum(r['native_matches']for r in rows),failed=sum(not r['native_matches']for r in rows),rows=rows,binary_sha256={str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in [go,parent,native]},scope='actual frozen Go/archived accepted parent/current CLI processes with newly native literal file credentials; only invalid argv/help; no provider HTTP')
pathlib.Path(prefix+'-cli-extra.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(receipt['cases'],receipt['passed'],receipt['failed'])
for r in rows:
 if not r['native_matches']:print(r['provider'],bytes.fromhex(r['arg_hex']),r['parent_matches'],base64.b64decode(r['go']['stderr_b64']).splitlines()[0],base64.b64decode(r['native']['stderr_b64']).splitlines()[0])
