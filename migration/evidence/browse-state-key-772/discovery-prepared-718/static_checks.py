from pathlib import Path
import builtins,gzip,hashlib,json,re,subprocess,symtable,tomllib
root=Path('/workspace/symaira-daemon772-state-key-discovery-clean')
def sha(data):return hashlib.sha256(data).hexdigest()
def digest(path):return sha(path.read_bytes())
def git(*args):return subprocess.check_output(['git',*args],cwd=root,text=True).strip()
head=git('rev-parse','HEAD'); assert not git('status','--porcelain')
assert subprocess.run(['git','merge-base','--is-ancestor','5e232700bb9031fc34d4995465a4037837540abb',head],cwd=root).returncode==0
proofroot=root/'migration/evidence/browse-state-key-772/independent-ccc'
receipt=json.loads((proofroot/'receipt.json').read_text()); proofs=[]
for row in receipt['raw_proofs']:
 data=(proofroot/row['retained']).read_bytes(); raw=gzip.decompress(data)
 assert sha(data)==row['gzip_sha256'] and sha(raw)==row['sha256'] and len(raw)==row['bytes']
 proofs.append({'retained':row['retained'],'raw_sha256':sha(raw),'gzip_sha256':sha(data)})
assert len(proofs)==57
standalone='browse/crates/symbrowse-core/src/key_sources/command/standalone.rs'
old=subprocess.check_output(['git','show','2b2755231b4ec5ce79de6fb2f022ac423abb44bd:'+standalone],cwd=root)
assert old==(root/standalone).read_bytes()
assert sha(old)=='28ace6936b05bf1d347feb1b442a94964663539bbfd5ce3738a45ac96d6af312'
harness=root/'browse/port/harness/daemon_provider_discovery.py';text=harness.read_text();compile(text,str(harness),'exec')
t=symtable.symtable(text,str(harness),'exec');defined={s.get_name() for s in t.get_symbols() if s.is_assigned() or s.is_imported()};used=set()
def visit(table):
 for s in table.get_symbols():
  if s.is_referenced() and s.is_global():used.add(s.get_name())
 for child in table.get_children():visit(child)
visit(t);missing=sorted(used-defined-set(dir(builtins))-{'__file__'});assert not missing
sdk=Path('/workspace/toolchains/go1.26.7/src/unicode/tables.go');s=sdk.read_text();assert 'const Version = "15.0.0"' in s
ranges=s.split('var _CaseRanges = []CaseRange{',1)[1].split('\n}',1)[0]
expected=[]
for low,high,deltas in re.findall(r'\{(0x[0-9A-Fa-f]+), (0x[0-9A-Fa-f]+), d\{([^}]+)\}\}',ranges):
 lower=deltas.split(',')[1].strip();delta=0x110000 if lower=='UpperLower' else int(lower)
 if delta:expected.append((int(low,16),int(high,16),delta))
lower=root/'browse/crates/symbrowse-core/src/key_sources/startup_discovery/windows_lower.rs'
actual=[(int(lo,16),int(hi,16),int(d)) for lo,hi,d in re.findall(r'\((0x[0-9A-Fa-f]+), (0x[0-9A-Fa-f]+), (-?\d+)\)',lower.read_text())]
assert len(expected)==187 and actual==expected
cargo=tomllib.loads((root/'browse/crates/symbrowse-core/Cargo.toml').read_text());assert cargo['target']['cfg(windows)']['dependencies']['same-file']=='=1.0.6'
packages=tomllib.loads((root/'browse/Cargo.lock').read_text())['package']
locks={n:next(p for p in packages if p['name']==n) for n in ['same-file','winapi-util']}
assert locks['same-file']['version']=='1.0.6' and locks['same-file']['checksum']=='93fc1dc3aaa9bfed95e02e6eadabb4baf7e3078b0bd1b4d7b6b0b68378900502'
assert locks['winapi-util']['version']=='0.1.11' and locks['winapi-util']['dependencies']==['windows-sys 0.61.2']
changed=git('diff','--name-only','2b2755231b4ec5ce79de6fb2f022ac423abb44bd',head).splitlines()
newfiles=[p for p in changed if p.startswith('browse/crates/') or p.startswith('browse/port/harness/') or p in ['browse/Cargo.lock','.github/workflows/browse-daemon-native.yml']]
sdkfiles=['src/internal/godebug/godebug.go','src/internal/bisect/bisect.go','src/path/filepath/path_unix.go','src/path/filepath/path_windows.go','src/internal/filepathlite/path.go','src/internal/filepathlite/path_windows.go','LICENSE','src/os/exec/lp_unix.go','src/os/exec/lp_windows.go','src/os/exec/exec.go','src/syscall/syscall_windows.go','src/unicode/tables.go']
report={'source_head':head,'source_clean':True,'current_main_ancestor':'5e232700bb9031fc34d4995465a4037837540abb','root_request_parent':'4798e5d099343e89d7a48dac916816df8a421510','original_state_publication':'2b2755231b4ec5ce79de6fb2f022ac423abb44bd','static_checks':{'rustfmt':True,'six_owned_go_fixtures_gofmt':True,'python_compile_no_execution':True,'unresolved_python_globals':missing,'actionlint':True,'all57_original_root_gzip_raw_sha':True,'standalone_byte_identity':sha(old),'sdk187_lower_ranges_exact':True,'manual_target_windows_lock_validation':locks},'candidate_source_sha256':{p:digest(root/p) for p in newfiles},'original_proofs':proofs,'sdk_source_sha256':{p:digest(Path('/workspace/toolchains/go1.26.7')/p) for p in sdkfiles},'prepared_scope':{'unix_cli_pairs':26,'windows_cli_pairs':35,'optional_original_retained_provider_pairs':5,'windows_actual_sdk_script_pairs':6,'typed_sdk_supported_settings':13,'explicit_unported_settings':8,'unix_actual_mutants':3,'windows_actual_mutants':2,'unix_new_unit_cases':4,'windows_new_unit_cases':6},'status':'STATIC PREPARATION ONLY: no Cargo/Go build, runtime/provider/port allocation, native proof, full acceptance or argv0 waiver. Compiler hold; old target absent. Fresh --locked compiler and all old/new actual gates plus different-author/native6 review required.'}
Path('/tmp/symaira-state-key-discovery-clean-718-static.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'source':head,'original_proofs':len(proofs),'candidate_files':len(newfiles),'lower_ranges':len(actual),'static':'PASS','runtime':'NOT RUN'}))
