from pathlib import Path
import hashlib,json,os,shutil,subprocess,tempfile
repo=Path('/workspace/symaira-usage768-remaining');root=Path('/tmp/symaira-usage768-remaining-go-owned');root.mkdir();output=Path('/tmp/symaira-usage768-remaining-device-go.json');source='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c';sha=lambda data:hashlib.sha256(data).hexdigest()
with tempfile.TemporaryDirectory(prefix='usage768-remaining-frozen-')as directory:
 worktree=Path(directory)/'source';subprocess.run(['git','worktree','add','--quiet','--detach',str(worktree),source],cwd=repo,check=True)
 try:
  helper=repo/'scripts/usage-remaining-baseline/provider_test.go.txt';shutil.copyfile(helper,worktree/'internal/usage/remaining_device_768_test.go')
  env=os.environ.copy();env.update(GOTOOLCHAIN='go1.26.7',CGO_ENABLED='0',USAGE_REMAINING_INPUT='/tmp/symaira-usage768-remaining-device-input.json',USAGE_REMAINING_ROOT=str(root),USAGE_REMAINING_OUTPUT=str(output))
  command=['/workspace/toolchains/go1.26.7/bin/go','test','./internal/usage','-run','^TestRemainingDeviceBaseline768$','-count=1']
  p=subprocess.run(command,cwd=worktree,env=env,capture_output=True);Path('/tmp/symaira-usage768-remaining-device-go-test.log').write_bytes(p.stdout+p.stderr);assert p.returncode==0,(p.returncode,p.stdout,p.stderr)
  assert not subprocess.check_output(['git','diff','--name-only'],cwd=worktree)
  fields=['go.mod','go.sum',*map(lambda p:str(p.relative_to(worktree)),(worktree/'internal/usage').glob('*.go'))];hashes={}
  for name in fields:
   if name.endswith('remaining_device_768_test.go'):continue
   data=(worktree/name).read_bytes();assert data==subprocess.check_output(['git','show',source+':'+name],cwd=worktree);hashes[name]=sha(data)
  records=json.loads(output.read_text());assert len(records)==39 and all(row['read_only']for row in records)
  receipt=dict(oracle_source=source,tracked_source_unchanged=True,input_sha256=sha(Path(env['USAGE_REMAINING_INPUT']).read_bytes()),helper_sha256=sha(helper.read_bytes()),output_sha256=sha(output.read_bytes()),source_sha256=hashes,command=command,sdk=subprocess.check_output(['/workspace/toolchains/go1.26.7/bin/go','version'],text=True).strip(),cases=39,full_go_values=True,all_read_only=True,cargo_builds=0,native_acceptance=False,scope='Fresh actual frozenGo constructors/strategy-chain/full reports/request rawbytes against custom canned transport; not native200/wire/header-stack proof. Valid Unicode family proposal and retained controls. Synthetic credentials only; no hostKeychain or public provider HTTP.')
  Path('/tmp/symaira-usage768-remaining-device-go-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print('PASS39 complete frozenGo constructor/report/request/state cases, synthetic/canned/read-only; no Rust compilation')
 finally:subprocess.run(['git','worktree','remove','--force',str(worktree)],cwd=repo,check=True)
