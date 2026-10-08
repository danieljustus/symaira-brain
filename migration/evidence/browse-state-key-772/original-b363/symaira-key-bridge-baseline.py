from __future__ import annotations
import importlib.util,json,os,platform,shutil,subprocess,tempfile,time
from pathlib import Path
ROOT=Path('/workspace/symaira-daemon772-state-key')
spec=importlib.util.spec_from_file_location('registry',ROOT/'browse/port/harness/daemon_registry.py');registry=importlib.util.module_from_spec(spec);spec.loader.exec_module(registry)
FROZEN=ROOT/'browse/testdata/port/state';manifest=json.loads((FROZEN/'manifest.json').read_text())
GO=Path('/tmp/symaira-pr801-go-1.26.7');RUST=Path('/workspace/symaira-daemon772-registry/target/debug/symbrowse');SOURCE=Path('/workspace/oracles/daemon772-go-source')
KEY=manifest['key_hex'];OTHER='cd'*32
assert subprocess.check_output(['git','-C',str(SOURCE),'rev-parse','HEAD'],text=True).strip()==registry.process.GO_REF
assert not subprocess.check_output(['git','-C',str(SOURCE),'status','--porcelain'],text=True)
for case in manifest['cases']:assert registry.process.digest(FROZEN/case['path'])==case['sha256']

def observe(binary,case,provider):
 with tempfile.TemporaryDirectory(prefix='bk-',dir=registry.process.private_temporary_parent()) as temporary:
  root=Path(temporary);env=registry.environment(root);env['SYMBROWSE_NO_AUTOSTART']='1';session='key'+str(os.getpid());log=root/'queries.jsonl';owned_bin=root/'bin';owned_bin.mkdir(mode=0o700)
  if case['vault'] is not None:shutil.copyfile(provider,owned_bin/('symvault.exe' if os.name=='nt' else 'symvault'));(owned_bin/('symvault.exe' if os.name=='nt' else 'symvault')).chmod(0o700)
  if os.name=='posix' and platform.system()=='Darwin':shutil.copyfile(provider,owned_bin/'security');(owned_bin/'security').chmod(0o700)
  env.update(PATH=str(owned_bin),SYMBROWSE_KEY_PROBE_MODE=case['vault'] or '',SYMBROWSE_KEY_PROBE_LEDGER=str(log))
  if case['key'] is not None:env['SYMBROWSE_ENCRYPTION_KEY']=case['key']
  states=Path(env['XDG_STATE_HOME'])/'symbrowse/states';states.mkdir(parents=True,mode=0o700)
  for fixture in manifest['cases']:shutil.copyfile(FROZEN/fixture['path'],states/(fixture['name']+'.json'))
  if case.get('store_file'):shutil.rmtree(states);states.write_text('owned-blocking-file')
  endpoint=registry.harness.daemon_socket_path(Path(env['XDG_RUNTIME_DIR']),session)
  command=[str(binary),'daemon','--session',session]+case.get('output',[]);began=time.monotonic();child=subprocess.Popen(command,cwd=root,env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=(os.name=='posix'));ready=False;records=[];stopped=None
  try:
   deadline=began+22
   while child.poll() is None and time.monotonic()<deadline:
    try:
     ping=registry.harness.request(endpoint,{'cmd':'daemon.ping','session':session});ready=ping.get('success',False)
     if ready:break
    except OSError:pass
    time.sleep(.02)
   elapsed=time.monotonic()-began
   if ready:
    records.append({'request':{'cmd':'state.list','session':session},'response':registry.harness.request(endpoint,{'cmd':'state.list','session':session})})
    if not case.get('store_file'):
     before={p.name:registry.process.digest(p) for p in states.iterdir()}
     for fixture in manifest['cases']:
      frame={'cmd':'state.show','session':session,'args':{'name':fixture['name']}};records.append({'request':frame,'response':registry.harness.request(endpoint,frame)})
     after={p.name:registry.process.digest(p) for p in states.iterdir()};assert before==after,'read-only metadata rewrote persisted files'
     frame={'cmd':'state.clean','session':session};records.append({'request':frame,'response':registry.harness.request(endpoint,frame)})
     retained={p.name:registry.process.digest(p) for p in states.iterdir()}
    else:before=after=retained={}
    stopped=registry.harness.request(endpoint,{'cmd':'daemon.stop','session':session})
   if child.poll() is None and not ready:raise AssertionError('startup did not complete inside owned bound')
   stdout,stderr=child.communicate(timeout=10)
   text=(stdout+stderr).decode();assert 'fixture-secret' not in text
   for record in records:assert 'fixture-secret' not in json.dumps(record)
   queries=[json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
   return {'case':case['name'],'root':str(root),'arguments':command,'startup_ready':ready,'startup_elapsed_seconds':elapsed,'exit':child.returncode,'stdout':stdout.decode(),'stderr':stderr.decode(),'records':records,'queries':queries,'retained_before':before if ready else {},'retained_after_inspection':after if ready else {},'retained_after_clean':retained if ready else {},'stop':stopped}
  finally:registry.harness.kill_tree(child)

cases=[{'name':'no-key','key':None,'vault':None},{'name':'empty-key','key':'','vault':None},{'name':'environment-ab','key':KEY,'vault':None},{'name':'environment-wrong-cd','key':OTHER,'vault':None},{'name':'environment-trimmed-ab','key':' '+KEY+' \n','vault':None}]
for output,name in [([], 'text'),(['--json'],'json'),(['--output','yaml'],'yaml')]:cases.append({'name':'invalid-environment-'+name,'key':'not-a-key','vault':None,'output':output})
for mode in ['ab','cd','2','3','4','invalid','empty','timeout']:cases.append({'name':'vault-'+mode+'-environment-ab','key':OTHER if mode=='ab' else KEY,'vault':mode})
cases.append({'name':'state-directory-is-file','key':None,'vault':None,'store_file':True})
rows=[]
with tempfile.TemporaryDirectory(prefix='bk-tool-') as tools:
 provider=Path(tools)/('provider.exe' if os.name=='nt' else 'provider');built=subprocess.run(['/workspace/toolchains/go1.26.7/bin/go','build','-o',str(provider),'/tmp/symaira-key-bridge-provider.go'],capture_output=True,check=True)
 for case in cases:
  row={'input':case,'go':observe(GO,case,provider),'rust':observe(RUST,case,provider)};rows.append(row)
  print(case['name'], 'GoReady=',row['go']['startup_ready'],'RustReady=',row['rust']['startup_ready'],'GoQueries=',len(row['go']['queries']),'RustQueries=',len(row['rust']['queries']),flush=True)
 report={'baseline_rust_head':'b363762cb85b879630fecd3fd141cba1ae85f71c','source_head':subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip(),'source_dirty':bool(subprocess.check_output(['git','-C',str(ROOT),'status','--porcelain'])),'go_ref':registry.process.GO_REF,'platform':platform.platform(),'binaries_sha256':{'go':registry.process.digest(GO),'rust':registry.process.digest(RUST),'owned_provider':registry.process.digest(provider)},'probe_sha256':registry.process.digest(Path(__file__)),'provider_source_sha256':registry.process.digest(Path('/tmp/symaira-key-bridge-provider.go')),'go_version':subprocess.check_output(['/workspace/toolchains/go1.26.7/bin/go','version'],text=True).strip(),'frozen_manifest_sha256':registry.process.digest(FROZEN/'manifest.json'),'frozen_fixture_sha256':{c['path']:c['sha256'] for c in manifest['cases']},'cases':rows,'note':'Original failing observations, not approval. Inputs/keys are public disposable fixtures only. No production source/fixture writes. Both six persisted Go file sets are inspected without rewriting; explicit clean may remove expired owned copies.'}
 Path('/tmp/symaira-key-bridge-b363-baseline.json').write_text(json.dumps(report,indent=2)+'\n')
