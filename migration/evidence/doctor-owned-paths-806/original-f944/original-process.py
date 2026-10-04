from pathlib import Path
import base64,fcntl,importlib.util,json,os,subprocess,sys,tempfile,time
from identity import identity
ROOT=Path(__file__).resolve().parents[2];GO=Path(os.environ['DOCTOR_STDOUT_GO']);RUST=Path(os.environ['DOCTOR_STDOUT_RUST']);BASE=Path(os.environ.get('DOCTOR_STDOUT_BASELINE',str(RUST)))
sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'))
spec=importlib.util.spec_from_file_location('fifth_other_doctor_output',ROOT/'scripts/doctor-repair-oracle/replay.py');m=importlib.util.module_from_spec(spec);sys.modules[spec.name]=m;spec.loader.exec_module(m)
rows=[]
for baseline in ([False,True] if os.environ.get('DOCTOR_STDOUT_BASELINE_PROBE')=='1' else [False]):
 for route,args in [('json',['doctor','--json']),('human',['doctor']),('help',['doctor','--help'])]:
  for sink in ['closed-reader','partial-reader','dev-full']:
   row={'case':route+'-'+sink,'baseline':baseline}
   for label,binary in [('go',GO),('rust',BASE if baseline else RUST)]:
    with tempfile.TemporaryDirectory(prefix='fifth-doctor-other-owned-')as fixture:
     root=Path(fixture);env=m.legacy.prepare_root(root,GO);home=root/'long-home'
     for i in range(12):home=home/(str(i)+'x'*239)
     home.mkdir(parents=True);env['HOME']=str(home);env['SYMBRAIN_GO_BINARY']=str(root/'absent-fallback')
     start=time.time();before=m.filesystem(root,{},start,start);received=b'';capacity=None
     if sink=='dev-full':
      with open('/dev/full','wb',buffering=0)as output:result=subprocess.run([str(binary),*args],cwd=env['PROJECT'],env=env,stdout=output,stderr=subprocess.PIPE,timeout=30)
      code=result.returncode;error=result.stderr
     else:
      read,write=os.pipe();fcntl.fcntl(write,fcntl.F_SETPIPE_SZ,4096);capacity=fcntl.fcntl(write,fcntl.F_GETPIPE_SZ)
      if sink=='closed-reader':os.close(read)
      child=subprocess.Popen([str(binary),*args],cwd=env['PROJECT'],env=env,stdout=write,stderr=subprocess.PIPE);os.close(write)
      if sink=='partial-reader':
       while len(received)<128:
        part=os.read(read,128-len(received))
        if not part:break
        received+=part
       os.close(read)
      _,error=child.communicate(timeout=30);code=child.returncode
     end=time.time();row[label]={'exit':code,'raw_stderr_base64':base64.b64encode(error).decode(),'stderr_base64':base64.b64encode(error.replace(os.fsencode(root),b'<root>')).decode(),'stdout_prefix_base64':base64.b64encode(received.replace(os.fsencode(root),b'<root>')).decode(),'raw_stdout_prefix_base64':base64.b64encode(received).decode(),'pipe_capacity':capacity,'before':before,'after':m.filesystem(root,{},start,end),'binary_sha256':m.digest(binary)}
   row['differences']=[key for key in ['exit','stderr_base64','stdout_prefix_base64','before','after']if row['go'][key]!=row['rust'][key]];rows.append(row)
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'go_sha256':m.digest(GO),'rust_sha256':m.digest(RUST),'baseline_sha256':m.digest(BASE) if os.environ.get('DOCTOR_STDOUT_BASELINE_PROBE')=='1' else None,'cases':rows,'scope':'Actual nonfix Doctor JSON/human/help OS output boundaries on disposable long HOME/XDG/PROJECT, all files/modes/bytes;4KiB bounded partial reader consumes128; no operator state, publisher or credentials. Same archiveda66 inputs.'}
report.update(identity());Path(os.environ['DOCTOR_STDOUT_REPORT']).write_text(json.dumps(report,indent=2)+'\n');print([(r['case'],r['baseline'],r['go']['exit'],r['rust']['exit'],r['differences'])for r in rows]);assert all(not r['differences']for r in rows if not r['baseline']); historical=[r for r in rows if r['baseline']]; assert not historical or sum(bool(r['differences'])for r in historical)==6
