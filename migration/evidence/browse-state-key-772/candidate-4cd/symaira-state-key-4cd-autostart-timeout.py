import sys,os,json,time,tempfile,subprocess,signal
from pathlib import Path
ROOT=Path('/workspace/symaira-daemon772-state-key');sys.path.insert(0,str(ROOT/'browse/port/harness'));import daemon_state_key as gate
rows=[]
with tempfile.TemporaryDirectory(prefix='bk-parent-tools-') as tools:
 provider=gate.key_test_environment.build_provider(Path(tools),'/workspace/toolchains/go1.26.7/bin/go')
 for label,binary in [('go',Path('/tmp/symaira-pr801-go-1.26.7')),('rust',Path('/workspace/symaira-daemon772-registry/target/debug/symbrowse'))]:
  with tempfile.TemporaryDirectory(prefix='bk-parent-') as owned:
   root=Path(owned);env=gate.registry.environment(root);binroot=root/'bin';binroot.mkdir(mode=0o700);(binroot/'symvault').write_bytes(provider.read_bytes());(binroot/'symvault').chmod(0o700);ledger=root/'queries.jsonl';env.update(PATH=str(binroot),SYMBROWSE_KEY_PROBE_MODE='timeout',SYMBROWSE_KEY_PROBE_LEDGER=str(ledger),SYMBROWSE_ENCRYPTION_KEY=gate.KEY);session='pt'+str(os.getpid());begin=time.monotonic()
   child=subprocess.Popen([str(binary),'state','list','--session',session,'--json'],cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
   stdout,stderr=child.communicate(timeout=20);finished=time.monotonic();print(label, 'CLI', child.returncode, stdout, stderr, flush=True);queries=[json.loads(line) for line in ledger.read_text().splitlines()];assert len(queries)==1;pid=queries[0]['pid'];original_pidfd=os.pidfd_open(pid)
   def status():
    try:
     stat=Path('/proc')/str(pid)/'stat';parts=stat.read_text().split();return {'pid':pid,'state':parts[2],'parent':int(parts[3]),'executable':os.readlink(Path('/proc')/str(pid)/'exe') if parts[2]!='Z' else None}
    except (FileNotFoundError,ProcessLookupError):return {'pid':pid,'state':'gone'}
   initial=status();assert initial.get('state')=='gone' or initial.get('state')=='Z' or initial['executable']==str(binroot/'symvault')
   while time.monotonic()<begin+16.5:time.sleep(.05)
   later=status();row={'kind':label,'source':'4cd56e3e47338aef30e5d83ea554227ace70fe71','binary_sha256':gate.registry.process.digest(binary),'root':str(root),'arguments':child.args,'exit':child.returncode,'stdout':stdout.decode(),'stderr':stderr.decode(),'elapsed':finished-begin,'queries':queries,'provider_after_cli':initial,'provider_after_16_5_seconds':later};rows.append(row);Path('/tmp/symaira-state-key-4cd-autostart-timeout.json').write_text(json.dumps({'cases':rows},indent=2)+'\n')
   print(label,child.returncode,finished-begin,initial,later,flush=True)
   if later.get('state') not in ['gone','Z']:signal.pidfd_send_signal(original_pidfd,signal.SIGKILL)
   os.close(original_pidfd)
Path('/tmp/symaira-state-key-4cd-autostart-timeout.json').write_text(json.dumps({'cases':rows,'scope':'owned Linux fixture processes only; no Windows/Darwin runtime claim; targeted extra observation, candidate unmodified'},indent=2)+'\n')
