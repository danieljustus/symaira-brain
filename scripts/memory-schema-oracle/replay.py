#!/usr/bin/env python3
import argparse,hashlib,json,os,pathlib,sqlite3,subprocess,tempfile
parser=argparse.ArgumentParser()
parser.add_argument('--go',type=pathlib.Path,required=True)
parser.add_argument('--rust',type=pathlib.Path,required=True)
parser.add_argument('--report',type=pathlib.Path,required=True)
args=parser.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];go=args.go.resolve();rust=args.rust.resolve();records=[]
cols=['embedding_binary','embedding_dim','embedding_quantization','lsh_hash','consolidated_into_id']
with tempfile.TemporaryDirectory(prefix='memory649-doctor-') as temporary:
 base=pathlib.Path(temporary);env={'HOME':str(base/'home'),'USERPROFILE':str(base/'home'),'XDG_CONFIG_HOME':str(base/'config'),'XDG_DATA_HOME':str(base/'data'),'XDG_CACHE_HOME':str(base/'cache'),'PATH':'','TZ':'UTC','LANG':'C.UTF-8','SYMBRAIN_GO_BINARY':str(base/'missing-go')}
 for key in ['SystemRoot','windir']:
  if key in os.environ:env[key]=os.environ[key]
 for key in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']:pathlib.Path(env[key]).mkdir(parents=True,exist_ok=True)
 path=base/'data/symbrain/memory/default.db'
 p=subprocess.run([str(go),'memory','list','--db',str(path),'--json'],cwd=base,env=env,capture_output=True,timeout=30);assert p.returncode==0,(p.stdout,p.stderr);path.chmod(0o600)
 def run(label,expected_missing):
  before=hashlib.sha256(path.read_bytes()).hexdigest();out=[]
  for binary in [go,rust]:
   p=subprocess.run([str(binary),'doctor','--json'],cwd=base,env=env,capture_output=True,timeout=30);assert p.returncode==0,(binary,p.stderr);d=json.loads(p.stdout)['memory_db'];out.append(d);assert hashlib.sha256(path.read_bytes()).hexdigest()==before,'Doctor mutated DB'
  if not expected_missing:assert out[0]==out[1],out
  else:
   assert 'error' not in out[0],out[0];assert out[0]['quick_check']=='ok';assert out[1].get('error')=='missing required columns: '+', '.join('memories.'+c for c in sorted(expected_missing)),out[1]
   p=subprocess.run([str(rust),'doctor'],cwd=base,env=env,capture_output=True,timeout=30);line=next(l for l in p.stdout.decode().splitlines() if 'memory db' in l);assert '✗' in line and 'missing required columns' in line
  records.append({'case':label,'go':out[0],'native':out[1],'db_sha256_before_after':before,'diagnostic_difference_intentional':bool(expected_missing)})
 run('healthy-actual-Go-created-store',[])
 if os.name == 'nt':
  path.chmod(0o444)
  try:run('healthy-Windows-readonly-attribute',[])
  finally:path.chmod(0o600)
 conn=sqlite3.connect(path)
 for name,sql in conn.execute("SELECT name,sql FROM sqlite_master WHERE type='index' AND tbl_name='memories'").fetchall():
  if sql and any(c in sql for c in cols):conn.execute('DROP INDEX "'+name.replace('"','""')+'"')
 for col in cols:conn.execute('ALTER TABLE memories DROP COLUMN '+col)
 conn.commit();conn.close();path.chmod(0o600)
 run('all-migrations-applied-but-five-columns-missing',cols)
 p=subprocess.run([str(rust),'memory','list','--db',str(path),'--json'],cwd=base,env=env,capture_output=True,timeout=30);assert p.returncode==0,(p.stdout,p.stderr)
 run('native-open-repairs-existing-store',[])
report={'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'candidate_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=root)),'local_platform':__import__('platform').platform(),'actual_go_constructor':'memory list --db owned/default.db --json; production migration constructor','actual_go_revision':'dcddcef0df5789123c7c9a7ebe6e01f10e941f2c','binary_sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [go,rust]},'records':records,'operator_state_used':False}
args.report.write_text(json.dumps(report,indent=2)+'\n');print(f'{len(records)} actual Go/native Doctor observations, retained legacy bug and corrected read-only diagnosis')
