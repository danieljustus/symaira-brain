#!/usr/bin/env python3
"""Actual Linux output/state boundary; retained independent inputs, no assertion weakening."""
from pathlib import Path
import base64,datetime,fcntl,hashlib,importlib.util,json,os,stat,subprocess,sys,tempfile,time
ROOT=Path(__file__).resolve().parents[2];GO=Path(os.environ["SETUP_OUTPUT_GO"]);RUST=Path(os.environ["SETUP_OUTPUT_RUST"])
def load(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);sys.modules[name]=m;spec.loader.exec_module(m);return m
setup=load('fourth_pipe_setup',ROOT/'scripts/setup-repair-oracle/replay.py');source=load('fourth_pipe_source',ROOT/'scripts/setup-source-oracle/replay.py')
def state(root,earliest,latest):
 result={}
 for p in sorted(root.rglob('*')):
  mode=stat.S_IMODE(p.lstat().st_mode);name=p.relative_to(root).as_posix()
  if p.is_symlink():result[name]=['link',mode,str(p.readlink())]
  elif p.is_dir():result[name]=['dir',mode]
  else:
   b=p.read_bytes().replace(os.fsencode(root),b'<root>')
   if name.endswith('.provenance.json'):
    v=json.loads(b);ts=v['built_at'];t=datetime.datetime.fromisoformat(ts.replace('Z','+00:00')).timestamp();assert ts.endswith('Z')and earliest-1<=t<=latest+1
    b=b.replace(ts.encode(),b'<validated-install-timestamp>')
   result[name]=['file',mode,len(b),hashlib.sha256(b).hexdigest(),base64.b64encode(b).decode()if len(b)<4096 else None]
 return result
rows=[]
with tempfile.TemporaryDirectory(prefix='fourth-pipe-tools-')as temporary,setup.legacy.ReleaseFixtureServer()as url:
 setup.legacy.RELEASE_BASE_URL=url;tmp=Path(temporary);src=tmp/'fixture.go';src.write_bytes((ROOT/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=tmp/'tool';subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True)
 for mode in ['closed-reader','partial-reader']:
  for route in ['install','fix','source']:
   row={'case':mode+'-'+route,'reader_bytes_requested':128 if mode=='partial-reader'else 0}
   for label,binary in [('go',GO),('rust',RUST)]:
    with tempfile.TemporaryDirectory(prefix='fourth-pipe-case-')as fixture:
     root=Path(fixture)
     if route=='source':env,args=source.configure(source.cases()[0],root,GO,tool)
     else:
      env=setup.legacy.prepare_root(root,GO);setup.legacy.setup_release_fixture(root,env);args=['setup']+(['--fix']if route=='fix'else[])+['--allow-unsigned','--json']
     raw=os.fsdecode(b'home\xff\xef\xbf\xbd\xe2\x82&<>\xe2\x80\xa8\xe2\x80\xa9');home=root
     if mode=='partial-reader':
      for i in range(12):home=home/(str(i)+'x'*239)
     home=home/raw;home.mkdir(parents=True);env['HOME']=str(home)+'/./';env['SYMBRAIN_GO_BINARY']=str(root/'absent-go')
     if mode=='partial-reader':(home/'.symaira').write_bytes(b'owned ancestor obstruction to produce non-atomic JSON report')
     earliest=time.time();before=source.filesystem(root,{},earliest,earliest,'7650123456789abcdef0123456789abcdef0123456')if route=='source'else state(root,earliest,earliest)
     read_fd,write_fd=os.pipe();fcntl.fcntl(write_fd,fcntl.F_SETPIPE_SZ,4096)
     if mode=='closed-reader':os.close(read_fd)
     child=subprocess.Popen([str(binary),*args],cwd=env['PROJECT'],env=env,stdout=write_fd,stderr=subprocess.PIPE);os.close(write_fd);prefix=b''
     if mode=='partial-reader':
      while len(prefix)<128:
       part=os.read(read_fd,128-len(prefix))
       if not part:break
       prefix+=part
      os.close(read_fd)
     _,stderr=child.communicate(timeout=20);latest=time.time()
     after=source.filesystem(root,{},earliest,latest,'7650123456789abcdef0123456789abcdef0123456')if route=='source'else state(root,earliest,latest)
     row[label]={'exit':child.returncode,'raw_stderr_base64':base64.b64encode(stderr).decode(),'stderr_base64':base64.b64encode(stderr.replace(os.fsencode(root),b'<root>')).decode(),'raw_received_prefix_base64':base64.b64encode(prefix).decode(),'received_prefix_base64':base64.b64encode(prefix.replace(os.fsencode(root),b'<root>')).decode(),'before':before,'after':after}
   row['differences']=[k for k in ['exit','stderr_base64','received_prefix_base64','before','after']if row['go'][k]!=row['rust'][k]];rows.append(row)
 result={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'go_sha256':source.digest(GO),'rust_sha256':source.digest(RUST),'tool_sha256':source.digest(tool),'scope':'Actual stdout pipe closed before spawn, or4KiB capacity with128 bytes consumed before closure and >4KiB ancestor-error JSON. Full owned state compared; completed writes preserved; only fixture roots/new validated UTC timestamps/staging names normalize.','cases':rows}
 output=Path(os.environ["SETUP_OUTPUT_REPORT"]);output.write_text(json.dumps(result,indent=2)+'\n')
 print([(r['case'],r['go']['exit'],r['rust']['exit'],r['differences'])for r in rows]);assert all(not r['differences']for r in rows)
