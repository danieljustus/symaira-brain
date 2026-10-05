#!/usr/bin/env python3
"""Actual Doctor child mutants: signal, concrete errno and ignored human errors."""
from pathlib import Path
import base64,json,os,subprocess,sys,tempfile
from identity import identity
ROOT=Path(__file__).resolve().parents[2];native=Path(os.environ['DOCTOR_STDOUT_RUST']).resolve();report=Path(os.environ['DOCTOR_STDOUT_REPORT']);rows=[]
with tempfile.TemporaryDirectory(prefix='doctor-stdout-controls-')as temporary:
 wrapper=Path(temporary)/'control';template='#!'+sys.executable+'\n'+'''
import os,signal,stat,subprocess,sys
child=subprocess.run([NATIVE,*sys.argv[1:]],stderr=subprocess.PIPE)
error=child.stderr
if MODE=='diagnostic':error=error.replace(b'write /dev/stdout: no space left on device',b'owned altered output error')
if MODE=='human-error' and sys.argv[1:]==['doctor'] and stat.S_ISCHR(os.fstat(1).st_mode) and os.fstat(1).st_rdev==os.stat('/dev/full').st_rdev:
 os.write(2,error+b'symbrain doctor: owned altered human error\\n');sys.exit(1)
os.write(2,error)
if child.returncode<0:
 if MODE=='signal' and child.returncode==-signal.SIGPIPE:sys.exit(0)
 signal.signal(-child.returncode,signal.SIG_DFL);os.kill(os.getpid(),-child.returncode)
sys.exit(child.returncode)
'''
 for mode,count,fields in [('diagnostic',1,['stderr_base64']),('signal',4,['exit']),('human-error',1,['exit','stderr_base64'])]:
  text=template.replace('NATIVE',repr(str(native))).replace('MODE',repr(mode));wrapper.write_text(text);wrapper.chmod(0o755)
  output=Path(temporary)/(mode+'.json');env={**os.environ,'DOCTOR_STDOUT_RUST':str(wrapper),'DOCTOR_STDOUT_REPORT':str(output),'DOCTOR_STDOUT_BASELINE_PROBE':'0'}
  run=subprocess.run([sys.executable,str(ROOT/'scripts/doctor-stdout-oracle/process.py')],env=env,capture_output=True,timeout=180)
  observed=json.loads(output.read_bytes());changed=[row for row in observed['cases']if row['differences']]
  report.write_text(json.dumps({'failed_control':observed,'stdout_base64':base64.b64encode(run.stdout).decode(),'stderr_base64':base64.b64encode(run.stderr).decode()},indent=2)+'\n')
  assert run.returncode!=0 and len(observed['cases'])==9 and len(changed)==count and all(row['differences']==fields for row in changed),(mode,run.stdout,run.stderr,[(r['case'],r['differences'])for r in changed])
  rows.append({'mode':mode,'expected_fields':fields,'actual_rejected_cases':count,'preserved_cases':9-count,'wrapper_text':text,'wrapper_sha256':__import__('hashlib').sha256(wrapper.read_bytes()).hexdigest(),'exit':run.returncode,'full_observations':observed,'stdout_base64':base64.b64encode(run.stdout).decode(),'stderr_base64':base64.b64encode(run.stderr).decode()})
 result={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'actual_native_sha256':__import__('hashlib').sha256(native.read_bytes()).hexdigest(),'rejected_groups':3,'groups':rows,'scope':'Actual native child performs complete owned work and writes; wrappers alter only terminal SIGPIPE status, JSON errno text, or human dev/full status/error. Unmodified cases remain equal; no assertion weakening.'}
 result.update(identity());report.write_text(json.dumps(result,indent=2)+'\n');print('3 actual Doctor groups reject6 intended changes and preserve21 equal observations')
