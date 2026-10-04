#!/usr/bin/env python3
"""Actual Linux output/state boundary; retained independent inputs, no assertion weakening."""
from pathlib import Path
import base64,hashlib,importlib.util,json,os,subprocess,tempfile,time
ROOT=Path(__file__).resolve().parents[2];GO=Path(os.environ["SETUP_OUTPUT_GO"]);RUST=Path(os.environ["SETUP_OUTPUT_RUST"])
spec=importlib.util.spec_from_file_location('source_output',ROOT/'scripts/setup-source-oracle/replay.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);rows=[]
with tempfile.TemporaryDirectory(prefix='source-fourth-output-tools-')as temporary:
 tmp=Path(temporary);src=tmp/'fixture.go';src.write_bytes((ROOT/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=tmp/'tool';subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True)
 for fault in [False,True]:
  row={'case':f'actual-dev-full-source-ancestor-{fault}'}
  for label,binary in [('go',GO),('rust',RUST)]:
   with tempfile.TemporaryDirectory(prefix='source-fourth-output-')as fixture:
    root=Path(fixture);case=dict(m.cases()[0],name=row['case']);env,args=m.configure(case,root,GO,tool)
    raw=os.fsdecode(b'home\xff\xef\xbf\xbd\xe2\x82&<>\xe2\x80\xa8\xe2\x80\xa9');home=root/raw;home.mkdir();env['HOME']=str(home)+'/../'+raw+'/./'
    if fault:(home/'.symaira').write_bytes(b'owned ancestor obstruction')
    started=time.time();before=m.filesystem(root,{},started,started,'7650123456789abcdef0123456789abcdef0123456')
    with open('/dev/full','wb',buffering=0)as output:completed=subprocess.run([str(binary),*args],cwd=env['PROJECT'],env=env,stdout=output,stderr=subprocess.PIPE,timeout=20)
    ended=time.time();row[label]={'exit':completed.returncode,'raw_stderr_base64':base64.b64encode(completed.stderr).decode(),'stderr_base64':base64.b64encode(m.normalize(completed.stderr,root)).decode(),'before':before,'after':m.filesystem(root,{},started,ended,'7650123456789abcdef0123456789abcdef0123456')}
  row['differences']=[k for k in ['exit','stderr_base64','before','after']if row['go'][k]!=row['rust'][k]];rows.append(row)
 result={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'go_sha256':m.digest(GO),'rust_sha256':m.digest(RUST),'tool_sha256':m.digest(tool),'cases':rows}
 Path(os.environ["SETUP_OUTPUT_REPORT"]).write_text(json.dumps(result,indent=2)+'\n');print([(row['case'],row['go']['exit'],row['rust']['exit'],row['differences'])for row in rows])
 assert all(not r['differences']and r['go']['exit']==1 for r in rows)
