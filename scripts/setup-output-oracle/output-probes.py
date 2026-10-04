#!/usr/bin/env python3
"""Actual Linux output/state boundary; retained independent inputs, no assertion weakening."""
from pathlib import Path
import base64,datetime,hashlib,importlib.util,json,os,stat,subprocess,sys,tempfile,time
ROOT=Path(__file__).resolve().parents[2];GO=Path(os.environ["SETUP_OUTPUT_GO"]);RUST=Path(os.environ["SETUP_OUTPUT_RUST"])
spec=importlib.util.spec_from_file_location('output_setup',ROOT/'scripts/setup-repair-oracle/replay.py');m=importlib.util.module_from_spec(spec);sys.modules[spec.name]=m;spec.loader.exec_module(m)
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
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
with m.legacy.ReleaseFixtureServer()as url:
 m.legacy.RELEASE_BASE_URL=url
 for fix in [False,True]:
  for fault in [None,'parent']:
   row={'case':f'actual-dev-full-setup-fix-{fix}-ancestor-{fault}'}
   for label,binary in [('go',GO),('rust',RUST)]:
    with tempfile.TemporaryDirectory(prefix='doctor-fourth-output-')as fixture:
     root=Path(fixture);env=m.legacy.prepare_root(root,GO);m.escaping_fixture(raw=True,lexical=True,fault=fault)(root,env)
     tmp=root/'owned-tmp';tmp.mkdir();env.update(TMPDIR=str(tmp),TMP=str(tmp),TEMP=str(tmp),SYMBRAIN_GO_BINARY=str(root/'absent-fallback'))
     started=time.time();before=state(root,started,started)
     args=['setup']+(['--fix']if fix else[])+['--allow-unsigned','--json']
     with open('/dev/full','wb',buffering=0)as output:
      completed=subprocess.run([str(binary),*args],cwd=env['PROJECT'],env=env,stdout=output,stderr=subprocess.PIPE,timeout=20)
     ended=time.time();row[label]={'exit':completed.returncode,'raw_stderr_base64':base64.b64encode(completed.stderr).decode(),'stderr_base64':base64.b64encode(completed.stderr.replace(os.fsencode(root),b'<root>')).decode(),'before':before,'after':state(root,started,ended)}
   row['differences']=[k for k in ['exit','stderr_base64','before','after']if row['go'][k]!=row['rust'][k]];rows.append(row)
receipt={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'go_sha256':digest(GO),'rust_sha256':digest(RUST),'cases':rows,'scope':'Actual Unix /dev/full stdout; full owned filesystem/modes/bytes and exit/stderr, validate newly installed UTC timestamps before normalizing only timestamp/fixture root.'}
Path(os.environ["SETUP_OUTPUT_REPORT"]).write_text(json.dumps(receipt,indent=2)+'\n')
print([(r['case'],r['go']['exit'],r['rust']['exit'],r['differences'])for r in rows]);assert all(not r['differences']and r['go']['exit']==1 for r in rows)
