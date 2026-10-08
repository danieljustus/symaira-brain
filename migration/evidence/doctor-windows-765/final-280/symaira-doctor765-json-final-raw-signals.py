import importlib.util,pathlib,sys,os,json,subprocess,tempfile,base64,time,signal,hashlib
ROOT=pathlib.Path('/workspace/symaira-doctor765-json')
spec=importlib.util.spec_from_file_location('setupsource',ROOT/'scripts/setup-source-oracle/replay.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
go=pathlib.Path('/tmp/symaira-setup765-source-independent-go');rust=pathlib.Path('/workspace/symaira-setup765-source/target/debug/symbrain');records=[]
with tempfile.TemporaryDirectory(prefix='setup765-independent-extra-') as scratch:
 scratch=pathlib.Path(scratch);src=scratch/'fixture.go';text=(ROOT/'scripts/setup-source-oracle/tool_fixture.go.txt').read_text();needle='\tvar target string\n';assert text.count(needle)==1;text=text.replace(needle,'\tif os.Getenv("SOURCE_HOLD_PARENT") != "" && action == "build" { time.Sleep(60 * time.Second) }\n'+needle);src.write_text(text);tool=scratch/'tool';subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True)
 cases=[{'name':'module-raw-separate','modules':b'bad\xff\xe2\x82'}, {'name':'module-raw-inline','args':['setup','--from-source','<source>',os.fsdecode(b'--modules=bad\xff\xe2\x82'),'--json']}, {'name':'missing-root-raw','args':['setup','--from-source','<source>/'+os.fsdecode(b'missing\xff\xe2\x82'),'--modules','browse','--json']}, {'name':'unknown-raw-after-source','args':['setup','--from-source','<source>',os.fsdecode(b'--bad\xff\xe2\x82')]}, {'name':'bool-raw-after-source','args':['setup','--from-source','<source>',os.fsdecode(b'--json=bad\xff\xe2\x82')]}]
 for case in cases:
  case.setdefault('args',['setup','--from-source','<source>','--modules','browse','--json']);item={'case':case['name']}
  for label,binary in [('go',go),('rust',rust)]:
   with tempfile.TemporaryDirectory(prefix='setup765-extra-case-') as fixture:item[label]=m.observe(binary,case,pathlib.Path(fixture),go,tool)
  item['mismatched_fields']=[key for key in item['go']['contract'] if item['go']['contract'][key]!=item['rust']['contract'][key]];records.append(item)
 original_configure=m.configure
 def raw_configure(case,root,go,tool):
  env,args=original_configure(case,root,go,tool);original=str(root/'receiving sources');selected=os.fsdecode(os.fsencode(original)+b'/owned\xff\xe2\x82');pathlib.Path(selected).mkdir();(pathlib.Path(selected)/'browse').mkdir();args=[arg.replace(original,selected) for arg in args];return env,args
 m.configure=raw_configure
 item={'case':'existing-root-raw-json'}
 for label,binary in [('go',go),('rust',rust)]:
  with tempfile.TemporaryDirectory(prefix='setup765-extra-case-') as fixture:item[label]=m.observe(binary,m.cases()[0],pathlib.Path(fixture),go,tool)
 item['mismatched_fields']=[key for key in item['go']['contract'] if item['go']['contract'][key]!=item['rust']['contract'][key]];records.append(item);m.configure=original_configure
 lifecycle=[]
 for sig in [signal.SIGINT,signal.SIGTERM]:
  with tempfile.TemporaryDirectory(prefix='setup765-extra-cancel-') as fixture:
   root=pathlib.Path(fixture);env,args=m.configure(m.cases()[0],root,go,tool);pidfile=root/'descendant.pid';env.update(SOURCE_DESCENDANT_PID=str(pidfile),SOURCE_HOLD_PARENT='true')
   proc=subprocess.Popen([str(rust),*args],cwd=env['PROJECT'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE);deadline=time.monotonic()+5
   while not pidfile.exists() and time.monotonic()<deadline:time.sleep(.02)
   assert pidfile.exists(),'held fixture never started';pid=int(pidfile.read_text());start=time.monotonic();proc.send_signal(sig);out,err=proc.communicate(timeout=5)
   status=pathlib.Path(f'/proc/{pid}/stat');active=status.exists() and not status.read_text().split(') ',1)[1].startswith('Z ')
   assert not active and proc.returncode==1 and b'source build cancelled' in out,(sig,active,proc.returncode,out,err)
   lifecycle.append({'signal':sig.name,'actual_cli_exit':proc.returncode,'descendant_pid':pid,'descendant_active':active,'elapsed_after_signal':time.monotonic()-start,'stdout_base64':base64.b64encode(out).decode(),'stderr_base64':base64.b64encode(err).decode()})
 receipt={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'candidate_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),'oracle_ref':m.ORACLE,'go_sdk':subprocess.check_output(['go','version'],text=True).strip(),'go_binary_sha256':m.digest(go),'rust_binary_sha256':m.digest(rust),'tool_binary_sha256':m.digest(tool),'raw_argv_observations':records,'actual_cancellation':lifecycle}
 pathlib.Path('/tmp/symaira-doctor765-json-final-raw-signals.json').write_text(json.dumps(receipt,indent=2)+'\n')
 print('raw cases',len(records),'mismatches',[(x['case'],x['mismatched_fields']) for x in records],'signals passed',len(lifecycle))
