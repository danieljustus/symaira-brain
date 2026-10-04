#!/usr/bin/env python3
"""Actual child controls reject missing diagnostic and missing signal status."""
from pathlib import Path
import base64,hashlib,json,os,subprocess,sys,tempfile
ROOT=Path(__file__).resolve().parents[2]
native=Path(os.environ['SETUP_OUTPUT_RUST']).resolve();report=Path(os.environ['SETUP_OUTPUT_REPORT']);rows=[]
with tempfile.TemporaryDirectory(prefix='setup-output-controls-')as temporary:
 wrapper=Path(temporary)/'control';template='#!'+sys.executable+'\n'+'''
import os,signal,subprocess,sys
child=subprocess.run([NATIVE,*sys.argv[1:]],stderr=subprocess.PIPE)
error=child.stderr
if MODE=='diagnostic':
 error=error.replace(b'write /dev/stdout: no space left on device',b'owned altered output error')
os.write(2,error)
if child.returncode<0:
 if MODE=='signal':sys.exit(0)
 signal.signal(-child.returncode,signal.SIG_DFL);os.kill(os.getpid(),-child.returncode)
sys.exit(child.returncode)
'''
 for mode,probes in [('diagnostic',['output-probes','source-output']),('signal',['pipe-probes','human-pipe-probes'])]:
  for name in probes:
   wrapper.write_text(template.replace('NATIVE',repr(str(native))).replace('MODE',repr(mode)));wrapper.chmod(0o755)
   output=Path(temporary)/(mode+'-'+name+'.json')
   env={**os.environ,'SETUP_OUTPUT_RUST':str(wrapper),'SETUP_OUTPUT_REPORT':str(output)}
   run=subprocess.run([sys.executable,str(ROOT/'scripts/setup-output-oracle'/(name+'.py'))],env=env,capture_output=True,timeout=180)
   observed=json.loads(output.read_bytes());cases=observed['cases'];changed=[case for case in cases if case['differences']]
   expected='stderr_base64'if mode=='diagnostic'else'exit'
   count=4 if name=='output-probes'else 2 if name=='source-output'else 6 if name=='pipe-probes'else 3
   report.write_text(json.dumps({'failed_control':observed},indent=2)+'\n')
   assert run.returncode!=0 and len(changed)==count and all(case['differences']==[expected]for case in changed),(mode,name,run.stdout,run.stderr)
   rows.append({'mode':mode,'probe':name,'exit':run.returncode,'exact_rejected_cases':count,'preserved_matching_cases':len(cases)-count,'stdout_base64':base64.b64encode(run.stdout).decode(),'stderr_base64':base64.b64encode(run.stderr).decode(),'actual_full_observations':observed})
 result={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'actual_native_sha256':hashlib.sha256(native.read_bytes()).hexdigest(),'wrapper_sha256':hashlib.sha256(wrapper.read_bytes()).hexdigest(),'wrapper_text':wrapper.read_text(),'rejected_groups':len(rows),'scope':'Actual native child keeps stdout/owned side effects; wrapper changes only error diagnostic or terminal signal status. No comparison weakening.','groups':rows}
 report.write_text(json.dumps(result,indent=2)+'\n');print('4 actual groups reject15 intended mismatches, preserve3 partial-human controls')
