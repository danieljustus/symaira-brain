#!/usr/bin/env python3
"""Actual Go/public-Rust Doctor writers, cumulative bytes and recovery."""
from pathlib import Path
import base64,dataclasses,importlib.util,json,os,subprocess,sys,tempfile,time
from identity import identity
ROOT=Path(__file__).resolve().parents[2]
GO=Path(os.environ['DOCTOR_STDOUT_GO']);RUST=Path(os.environ['DOCTOR_STDOUT_EMBEDDED']).resolve()
sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'))
spec=importlib.util.spec_from_file_location('doctor_embedded_state',ROOT/'scripts/doctor-repair-oracle/replay.py');m=importlib.util.module_from_spec(spec);sys.modules[spec.name]=m;spec.loader.exec_module(m)
rows=[]
with tempfile.TemporaryDirectory(prefix='doctor-writer-sdk-')as temporary,m.legacy.ReleaseFixtureServer()as url:
 m.legacy.RELEASE_BASE_URL=url;owned=Path(temporary);source=owned/'source';source.mkdir()
 archive=subprocess.Popen(['git','archive',m.ORACLE],cwd=ROOT,stdout=subprocess.PIPE)
 subprocess.run(['tar','-xf','-','-C',str(source)],stdin=archive.stdout,check=True);archive.stdout.close();assert archive.wait()==0
 driver=source/'cmd/symbrain/owned_doctor_output.go';driver.write_bytes((ROOT/'scripts/doctor-stdout-oracle/embedded.go.txt').read_bytes())
 embedded_go=owned/'embedded-go';subprocess.run(['go','build','-mod=readonly','-o',str(embedded_go),'./cmd/symbrain'],cwd=source,check=True)
 probe=m.build_probe(owned)
 for recover,budget in [(False,0),(False,7),(True,7)]:
  for mode in ['raw-pipe','raw-full','callback']:
   for route,args in [('fix',['doctor','--fix']),('json',['doctor','--json']),('human',['doctor'])]:
    case=dataclasses.replace(m.cases()[0],name=f'{mode}-{route}-{budget}-{recover}',verifier=True);row={'case':case.name,'route':route,'budget':budget,'recover':recover}
    for label,binary in [('go',embedded_go),('rust',RUST)]:
     with tempfile.TemporaryDirectory(prefix='doctor-writer-owned-')as fixture:
      root=Path(fixture);env=m.legacy.prepare_root(root,GO);m.configure(case,root,env,probe)
      env.update(DOCTOR_WRITER_MODE=mode,DOCTOR_WRITER_BUDGET=str(budget),DOCTOR_WRITER_RECOVER='1'if recover else'0',SYMBRAIN_GO_BINARY=str(root/'absent-fallback'))
      originals={p.relative_to(root).as_posix():m.legacy.normalize_fixture_root(p.read_bytes(),root)for p in root.rglob('*.provenance.json')if p.is_file()};start=time.time();before=m.filesystem(root,originals,start,start)
      result=subprocess.run([str(binary),*args],cwd=env['PROJECT'],env=env,capture_output=True,timeout=30);end=time.time()
      row[label]={'exit':result.returncode,'stdout_base64':base64.b64encode(result.stdout.replace(os.fsencode(root),b'<root>')).decode(),'raw_stdout_base64':base64.b64encode(result.stdout).decode(),'stderr_base64':base64.b64encode(result.stderr.replace(os.fsencode(root),b'<root>')).decode(),'raw_stderr_base64':base64.b64encode(result.stderr).decode(),'logs':m.log_contract(result.stderr,root,start,end,case)if route=='fix'else None,'before':before,'after':m.filesystem(root,originals,start,end)}
    fields=['exit','stdout_base64','before','after']+(['logs']if route=='fix'else['stderr_base64'])
    row['differences']=[key for key in fields if row['go'][key]!=row['rust'][key]];rows.append(row)
 report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'go_sha256':m.digest(embedded_go),'rust_sha256':m.digest(RUST),'extra_caller_sha256':m.digest(driver),'probe_sha256':m.digest(probe),'go_sdk':subprocess.check_output(['go','version'],text=True).strip(),'cases':rows,'scope':'Actual immutable Go Doctor/public Rust caller-owned writer; no actual stdout signal claim. Immediate/cumulative7-byte errors and recovering callbacks; complete files/modes/log attributes. Unix raw syscall messages require native Unix; Linux execution is not native Windows/macOS acceptance.'}
 report.update(identity());Path(os.environ['DOCTOR_STDOUT_REPORT']).write_text(json.dumps(report,indent=2)+'\n');print([(r['case'],r['go']['exit'],r['rust']['exit'],r['differences'])for r in rows]);assert len(rows)==27 and all(not r['differences']for r in rows)
