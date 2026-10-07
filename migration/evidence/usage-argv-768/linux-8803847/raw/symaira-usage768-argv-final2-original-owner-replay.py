from pathlib import Path
import hashlib,json,os,subprocess,tempfile
repo=Path('/workspace/symaira-usage768-argv')
go=Path('/tmp/symaira-usage768-f759-root-owner-go')
rust=Path('/tmp/symaira-usage768-argv-final2-public-probe')
rlib=repo/'target/debug/deps/libsymbrain_usage-bc5b4536881a808f.rlib'
output=[]
for original_name,field in [('owner-extra','pairs'),('literal-owner-extra','rows')]:
 original=Path('/tmp/symaira-usage768-f759-root-'+original_name+'.json');rows=json.loads(original.read_text())[field]
 for index,row in enumerate(rows):
  with tempfile.TemporaryDirectory(prefix='usage768-original-owner-replay-')as scratch:
   root=Path(scratch)
   for directory in ['home','plain','owner/nested','owner/home']:(root/directory).mkdir(parents=True,exist_ok=True)
   (root/'link').symlink_to(root/'owner/nested',target_is_directory=True)
   if field=='rows':inputs=row['input_bytes_hex']
   else:
    inputs={}
    for relative in row['before']:
     token='physical-owner'if relative.startswith('owner/')else'lexical-owner'
     value={'github.com:a':{'oauth_token':token}}if row['provider']=='copilot'else{'access_token':token}
     inputs[relative]=json.dumps(value).encode().hex()
     assert hashlib.sha256(bytes.fromhex(inputs[relative])).hexdigest()==row['before'][relative],'exact original ordinary input bytes'
   for relative,value in inputs.items():
    path=root/relative;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(bytes.fromhex(value))
   oldroot=str(Path(row['env']['SYMBRAIN_GO_BINARY']).parent)
   env={key:value.replace(oldroot,str(root))for key,value in row['env'].items()}
   before={relative:hashlib.sha256((root/relative).read_bytes()).hexdigest()for relative in inputs}
   observations=[]
   for binary in [go,rust]:
    process=subprocess.run([str(binary),row['provider']],env=env,capture_output=True,check=True,timeout=15)
    assert not process.stderr
    observations.append(json.loads(process.stdout))
   actual=observations[1];gate=actual.pop('needs_go');assert gate is False
   assert observations[0]==actual==row['go'],(original_name,index,observations,row['go'])
   after={relative:hashlib.sha256((root/relative).read_bytes()).hexdigest()for relative in inputs}
   assert before==after and (root/'link').readlink()==root/'owner/nested'
   output.append(dict(original_receipt=str(original),original_receipt_sha256=hashlib.sha256(original.read_bytes()).hexdigest(),original_index=index,provider=row['provider'],variant=row['variant'],input_bytes_hex=inputs,go=observations[0],native=actual,needs_go_fallback=gate,matches=True,read_only=True,before=before,after=after))
assert len(output)==12
receipt=dict(source=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),candidate_clean=not subprocess.check_output(['git','status','--porcelain'],cwd=repo,text=True).strip(),cases=12,passed=12,failed=0,records=output,binary_sha256={str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in [go,rust,rlib]},probe_source_sha256=hashlib.sha256(Path('/tmp/symaira-usage768-f759-root-owner-probe.rs').read_bytes()).hexdigest(),runner_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),scope='actual frozen Go constructors and public current all_providers selector with owned canned401 transport; Linux only')
Path('/tmp/symaira-usage768-argv-final2-original-owner-replay.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Original exact owner cases:12/12, correct lexical owner; all read-only')
