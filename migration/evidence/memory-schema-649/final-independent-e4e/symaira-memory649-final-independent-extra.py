import pathlib,tempfile,subprocess,sqlite3,json,hashlib
binary=pathlib.Path('/workspace/symaira-memory649-doctor/target/debug/symbrain');go=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0');records=[]
with tempfile.TemporaryDirectory(prefix='memory649-independent-') as td:
 root=pathlib.Path(td);env={'HOME':str(root/'home'),'USERPROFILE':str(root/'home'),'XDG_CONFIG_HOME':str(root/'config'),'XDG_DATA_HOME':str(root/'data'),'XDG_CACHE_HOME':str(root/'cache'),'PATH':'','SYMBRAIN_GO_BINARY':str(root/'missing-go')};db=root/'data/symbrain/memory/default.db'
 for key in ['HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME']:pathlib.Path(env[key]).mkdir(parents=True)
 p=subprocess.run([str(go),'memory','list','--db',str(db),'--json'],env=env,cwd=root,capture_output=True);assert p.returncode==0
 healthy=db.read_bytes()
 cases=['healthy','missing-table','single-column','empty','corrupt','directory','view-in-place-of-table','case-renamed-column']
 for case in cases:
  if db.is_dir():db.rmdir()
  db.write_bytes(healthy);db.chmod(0o600)
  if case in ['missing-table','single-column','view-in-place-of-table','case-renamed-column']:
   with sqlite3.connect(db) as conn:
    if case=='missing-table':conn.execute('DROP TABLE sessions')
    if case=='single-column':conn.execute('ALTER TABLE memories DROP COLUMN embedding_binary')
    if case=='view-in-place-of-table':conn.execute('DROP TABLE sessions');conn.execute("CREATE VIEW sessions AS SELECT '' AS id,'' AS summary,'' AS updated_at")
    if case=='case-renamed-column':conn.execute('ALTER TABLE memories RENAME COLUMN embedding_binary TO EMBEDDING_BINARY')
   conn.close()
  elif case=='empty':db.write_bytes(b'')
  elif case=='corrupt':db.write_bytes(b'not a database')
  elif case=='directory':db.unlink();db.mkdir()
  before=None if db.is_dir() else hashlib.sha256(db.read_bytes()).hexdigest()
  write_error=None
  if case=='view-in-place-of-table':
   conn=sqlite3.connect(db)
   try:conn.execute("INSERT INTO sessions(id,summary,updated_at) VALUES ('owned-probe','owned-probe','2026-10-03')")
   except sqlite3.OperationalError as error:write_error=str(error)
   finally:conn.close()
  outputs=[]
  for js in [True,False]:
   p=subprocess.run([str(binary),'doctor']+(['--json'] if js else []),env=env,cwd=root,capture_output=True,timeout=10)
   assert p.returncode==0 and not p.stderr,(case,p.returncode,p.stderr)
   value=json.loads(p.stdout)['memory_db'] if js else next(x for x in p.stdout.decode().splitlines() if 'memory db' in x)
   outputs.append(value)
   if before:assert hashlib.sha256(db.read_bytes()).hexdigest()==before
  records.append({'case':case,'json':outputs[0],'human_line':outputs[1],'main_database_unchanged':True,'actual_write_error':write_error})
pathlib.Path('/tmp/symaira-memory649-final-independent-extra.json').write_text(json.dumps({'candidate':'e4e06371b6f90dedb920072f36bdfa7231200dab','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'records':records},indent=2)+'\n')
for record in records:print(record['case'],record['json'].get('error','NO ERROR'))
