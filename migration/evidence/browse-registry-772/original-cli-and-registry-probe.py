import importlib.util, json, os, socket, subprocess, tempfile, threading, time
from pathlib import Path

worktree=Path('/workspace/symaira-daemon772-registry')
spec=importlib.util.spec_from_file_location('h',worktree/'browse/port/harness/daemon_process.py')
h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
binaries={'go':Path('/tmp/symaira-pr801-go-1.26.7'),'rust':Path('/workspace/symaira-daemon772/target/debug/symbrowse')}
results={}
for language,binary in binaries.items():
 with tempfile.TemporaryDirectory(prefix='br-') as directory:
  root=Path(directory);home=root/'home';home.mkdir();runtime=root/'run';runtime.mkdir()
  env={'HOME':str(home),'XDG_CONFIG_HOME':str(root/'config'),'XDG_CACHE_HOME':str(root/'cache'),'XDG_STATE_HOME':str(root/'state'),'XDG_RUNTIME_DIR':str(runtime),'SYMBROWSE_NO_AUTOSTART':'1','PATH':'','TMPDIR':str(root/'tmp')}
  result=[]
  for command in [['session','list'],['session','info'],['daemon','status']]:
   output=subprocess.run([str(binary),*command,'--session','missing','--json'],env=env,cwd=root,capture_output=True,timeout=5)
   result.append({'command':command,'exit':output.returncode,'stdout':output.stdout.decode(),'stderr':output.stderr.decode()})
  daemon=subprocess.Popen([str(binary),'daemon','--session','registry'],env=env,cwd=root,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
  endpoint=h.harness.daemon_socket_path(runtime,'registry')
  try:
   h.harness.wait_for_request(endpoint,{'cmd':'daemon.ping','session':'registry'})
   responses=[]
   for frame in [{'cmd':'session.info','session':'unknown'},{'cmd':'session.info','session':'registry'},{'cmd':'session.list','session':'registry'},{'cmd':'session.info','session':'registry'},{'cmd':'daemon.ping','session':'bad session!'}]:
    time.sleep(.02);responses.append({'frame':frame,'response':h.harness.request(endpoint,frame)})
   h.harness.request(endpoint,{'cmd':'daemon.stop','session':'registry'});out,err=daemon.communicate(timeout=5)
   result.append({'case':'registry','pid':daemon.pid,'responses':responses,'exit':daemon.returncode,'stdout':out.decode(),'stderr':err.decode()})
  except Exception:
   h.harness.kill_tree(daemon);out,err=daemon.communicate(timeout=3);print(language,daemon.returncode,out,err);raise
  finally:h.harness.kill_tree(daemon)
  if language=='rust':
   fake_path=h.harness.daemon_socket_path(runtime,'fixture');fake_path.parent.mkdir(parents=True,exist_ok=True)
   listener=socket.socket(socket.AF_UNIX);listener.bind(str(fake_path));listener.listen();listener.settimeout(.3);seen=[];stop=threading.Event()
   def serve():
    while not stop.is_set():
     try:connection,_=listener.accept()
     except TimeoutError:continue
     with connection:
      frame=json.loads(connection.makefile('rb').readline());seen.append(frame)
      response={'success':False,'error':{'code':'operation_failed','message':'status unavailable'}} if frame['cmd']=='daemon.status' else {'success':True,'data':{'schema_version':1,'sessions':[]}}
      connection.sendall(json.dumps(response).encode()+b'\n')
   thread=threading.Thread(target=serve);thread.start()
   try:
    output=subprocess.run([str(binary),'session','list','--session','fixture','--json'],env=env,cwd=root,capture_output=True,timeout=5)
    result.append({'case':'state-success-status-failure','requests':seen,'exit':output.returncode,'stdout':output.stdout.decode(),'stderr':output.stderr.decode()})
   finally:stop.set();thread.join();listener.close()
  results[language]={'binary':str(binary),'sha256':h.digest(binary),'root':str(root),'cases':result}
Path('/tmp/symaira-registry-original-probes.json').write_text(json.dumps(results,indent=2)+'\n')
print('retained original Go/native probes')
