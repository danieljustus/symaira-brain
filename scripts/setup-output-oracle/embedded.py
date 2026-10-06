#!/usr/bin/env python3
"""Actual frozen Go callers and public Rust API with ordinary owned writers."""
from pathlib import Path
import base64,importlib.util,json,os,subprocess,sys,tempfile,time
ROOT=Path(__file__).resolve().parents[2]
GO=Path(os.environ['SETUP_OUTPUT_GO']);RUST=Path(os.environ['SETUP_OUTPUT_EMBEDDED_RUST']).resolve()
def load(name,path):
 spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);sys.modules[name]=module;spec.loader.exec_module(module);return module
setup=load('embedded_setup_output',ROOT/'scripts/setup-repair-oracle/replay.py')
source=load('embedded_source_output',ROOT/'scripts/setup-source-oracle/replay.py')
state=load('embedded_output_state',ROOT/'scripts/setup-output-oracle/fixtures.py')
rows=[]
with tempfile.TemporaryDirectory(prefix='setup-embedded-sdk-')as temporary,setup.legacy.ReleaseFixtureServer()as url:
 setup.legacy.RELEASE_BASE_URL=url;owned=Path(temporary);go_source=owned/'source';go_source.mkdir()
 # Owned archived production package with one additional, explicit test caller.
 archive=subprocess.Popen(['git','archive',source.ORACLE],cwd=ROOT,stdout=subprocess.PIPE)
 subprocess.run(['tar','-xf','-','-C',str(go_source)],stdin=archive.stdout,check=True);archive.stdout.close();assert archive.wait()==0
 driver=go_source/'cmd/symbrain/owned_output.go';driver.write_bytes((ROOT/'scripts/setup-output-oracle/embedded.go.txt').read_bytes())
 embedded_go=owned/'embedded-go';subprocess.run(['go','build','-mod=readonly','-o',str(embedded_go),'./cmd/symbrain'],cwd=go_source,check=True)
 src=owned/'fixture.go';src.write_bytes((ROOT/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=owned/'tool';subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True)
 for mode in ['raw-pipe','raw-full','kind-pipe','callback']:
  for route in ['install','fix','source']:
   for human in [False,True]:
    row={'case':mode+'-'+route+('-human'if human else'-json')}
    for label,binary in [('go',embedded_go),('rust',RUST)]:
     with tempfile.TemporaryDirectory(prefix='setup-embedded-owned-')as fixture:
      root=Path(fixture)
      if route=='source':env,args=source.configure(source.cases()[0],root,GO,tool)
      else:
       env=setup.legacy.prepare_root(root,GO);setup.escaping_fixture(raw=True,lexical=True)(root,env)
       args=['setup']+(['--fix']if route=='fix'else[])+['--allow-unsigned','--json']
      if human:args=[arg for arg in args if arg!='--json']
      env['SETUP_EMBEDDED_MODE']=mode;env['SYMBRAIN_GO_BINARY']=str(root/'absent-fallback')
      started=time.time();before=source.filesystem(root,{},started,started,'7650123456789abcdef0123456789abcdef0123456')if route=='source'else state.state(root,started,started)
      result=subprocess.run([str(binary),*args],cwd=env['PROJECT'],env=env,capture_output=True,timeout=20);ended=time.time()
      after=source.filesystem(root,{},started,ended,'7650123456789abcdef0123456789abcdef0123456')if route=='source'else state.state(root,started,ended)
      row[label]={'exit':result.returncode,'stdout_base64':base64.b64encode(result.stdout).decode(),'raw_stderr_base64':base64.b64encode(result.stderr).decode(),'stderr_base64':base64.b64encode(result.stderr.replace(os.fsencode(root),b'<root>')).decode(),'before':before,'after':after}
    row['differences']=[key for key in ['exit','stdout_base64','stderr_base64','before','after']if row['go'][key]!=row['rust'][key]];rows.append(row)
 receipt={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'rust_sha256':source.digest(RUST),'go_sha256':source.digest(embedded_go),'extra_caller_sha256':source.digest(driver),'tool_sha256':source.digest(tool),'scope':'Actual public library/custom-writer Go/Rust callers, including raw syscall EPIPE; no process-stdout signal claim. Full committed files/modes/bytes retained; fixture roots and validated new clocks only normalize. Linux SDK execution, not native Windows/macOS.','cases':rows}
 Path(os.environ['SETUP_OUTPUT_REPORT']).write_text(json.dumps(receipt,indent=2)+'\n');print([(row['case'],row['go']['exit'],row['rust']['exit'],row['differences'])for row in rows])
 assert len(rows)==24 and all(not row['differences']for row in rows)
