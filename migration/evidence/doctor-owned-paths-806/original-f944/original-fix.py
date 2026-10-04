from pathlib import Path
import base64,hashlib,importlib.util,json,os,subprocess,sys,tempfile,time
from identity import identity
ROOT=Path(__file__).resolve().parents[2];GO=Path(os.environ['DOCTOR_STDOUT_GO']);RUST=Path(os.environ['DOCTOR_STDOUT_RUST']);BASE=Path(os.environ.get('DOCTOR_STDOUT_BASELINE',str(RUST)))
sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'))
spec=importlib.util.spec_from_file_location('independent_fifth_doctor_output',ROOT/'scripts/doctor-repair-oracle/replay.py');m=importlib.util.module_from_spec(spec);sys.modules[spec.name]=m;spec.loader.exec_module(m)
rows=[]
with tempfile.TemporaryDirectory(prefix='fifth-doctor-output-tools-')as tools,m.legacy.ReleaseFixtureServer()as url:
 m.legacy.RELEASE_BASE_URL=url;probe=m.build_probe(Path(tools))
 for baseline in ([False,True] if os.environ.get('DOCTOR_STDOUT_BASELINE_PROBE')=='1' else [False]):
  for sink in ['closed-reader','dev-full']:
   for missing in [False,True]:
    case=m.cases()[0];case=__import__('dataclasses').replace(case,name=f'{sink}-missing-{missing}',all_missing=missing,verifier=True)
    row={'case':case.name,'baseline':baseline}
    for label,binary in [('go',GO),('rust',BASE if baseline else RUST)]:
     with tempfile.TemporaryDirectory(prefix='fifth-doctor-output-owned-')as fixture:
      root=Path(fixture);env=m.legacy.prepare_root(root,GO);m.configure(case,root,env,probe);env['SYMBRAIN_GO_BINARY']=str(root/'absent-fallback')
      originals={p.relative_to(root).as_posix():m.legacy.normalize_fixture_root(p.read_bytes(),root)for p in root.rglob('*.provenance.json')if p.is_file()};start=time.time();before=m.filesystem(root,originals,start,start)
      if sink=='closed-reader':
       read,write=os.pipe();os.close(read)
       try:result=subprocess.run([str(binary),*case.args],cwd=env['PROJECT'],env=env,stdout=write,stderr=subprocess.PIPE,timeout=20)
       finally:os.close(write)
      else:
       with open('/dev/full','wb',buffering=0)as output:result=subprocess.run([str(binary),*case.args],cwd=env['PROJECT'],env=env,stdout=output,stderr=subprocess.PIPE,timeout=20)
      end=time.time();row[label]={'exit':result.returncode,'raw_stderr_base64':base64.b64encode(result.stderr).decode(),'logs':m.log_contract(result.stderr,root,start,end,case),'before':before,'after':m.filesystem(root,originals,start,end),'binary_sha256':m.digest(binary)}
    row['differences']=[key for key in ['exit','logs','before','after']if row['go'][key]!=row['rust'][key]];rows.append(row)
 report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'go_sha256':m.digest(GO),'rust_sha256':m.digest(RUST),'baseline_sha256':m.digest(BASE) if os.environ.get('DOCTOR_STDOUT_BASELINE_PROBE')=='1' else None,'probe_sha256':m.digest(probe),'cases':rows,'scope':'Actual Doctor --fix stdout sinks, full committed filesystem and log attributes; owned local release fixture/real verifier only; no operator state or rollback. Archived a66 same inputs; dev/full controls.'}
 report.update(identity());Path(os.environ['DOCTOR_STDOUT_REPORT']).write_text(json.dumps(report,indent=2)+'\n');print([(r['case'],r['baseline'],r['go']['exit'],r['rust']['exit'],r['differences'])for r in rows]);assert all(not r['differences']for r in rows if not r['baseline']); historical=[r for r in rows if r['baseline']]; assert not historical or sum(bool(r['differences'])for r in historical)==2
