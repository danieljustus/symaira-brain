from pathlib import Path
import gzip, hashlib, json, shutil, subprocess

root = Path('/workspace/symaira-memory803-open')
target = root / 'target'
scratch = Path('/tmp/symaira-memory803-focused-4c62355')
out = root / 'migration/evidence/memory-open-race-803/final-4c62355'
out.mkdir(exist_ok=True)
archive = Path('/workspace/oracles/symaira-memory803-4c62355-binaries')
archive.mkdir(exist_ok=True)
def sha(raw):
    return hashlib.sha256(raw).hexdigest()
def record(path):
    return sha(path.read_bytes())
def users():
    found = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        for link in [proc/'exe', proc/'cwd', *list((proc/'fd').glob('*'))]:
            try:
                value = str(link.resolve(strict=True))
            except (OSError, RuntimeError):
                continue
            if value == str(target) or value.startswith(str(target)+'/'):
                found.append({'pid':proc.name,'link':str(link),'value':value})
    return found
assert not users()
head = subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert head.startswith('4c62355')
old = json.loads((root/'migration/evidence/memory-open-race-803/final-bdf2371/validation.json').read_text())
previous = json.loads(Path(old['elf_receipt']).read_text())
by_sha = {item['sha256']:item['archive'] for item in previous['ELFs']}
checked = set()
elfs = []
for path in sorted(target.rglob('*')):
    if not path.is_file():
        continue
    with path.open('rb') as handle:
        if handle.read(4) != b'\x7fELF':
            continue
    raw = path.read_bytes()
    digest = sha(raw)
    if digest not in by_sha:
        compressed = archive/(digest+'.gz')
        compressed.write_bytes(gzip.compress(raw,compresslevel=9,mtime=0))
        by_sha[digest]={'gzip':str(compressed),'gzip_sha256':record(compressed),'bytes':len(raw)}
    item = by_sha[digest]
    if digest not in checked:
        data = Path(item['gzip']).read_bytes()
        assert sha(data) == item['gzip_sha256']
        assert sha(gzip.decompress(data)) == digest
        checked.add(digest)
    elfs.append({'path':str(path),'sha256':digest,'archive':item})
selected = next(x for x in elfs if x['path'].endswith('/deps/symbrain_memory-dde9bae26a7c26aa'))
assert sha((root/'rust/symbrain-memory/src/wal_open.rs').read_bytes()) == 'b7e9db536076793c9d1d5b608869dd9cb6b0dee9a90780022c168d5d30903cf9'
base = subprocess.check_output(['git','show','6ae73a7:rust/symbrain-memory/src/migration.rs'],cwd=root)
current = (root/'rust/symbrain-memory/src/migration.rs').read_bytes()
assert current.split(b'#[cfg(test)]',1)[1] == base.split(b'#[cfg(test)]',1)[1]
sources = {path:record(root/path) for path in old['source_files']}
sources.update({str(path.relative_to(root)):record(path) for path in (root/'.github/workflows').glob('*') if path.is_file()})
assert not subprocess.check_output(['git','diff','d751daa','HEAD','--','rust','Cargo.lock','Cargo.toml','.cargo'],cwd=root)
corekit = {path:record(Path(path)) for path in old['corekit_source_inputs']}
assert corekit == old['corekit_source_inputs']
windows = root/'migration/evidence/memory-open-race-803/windows-111341331340'
originals = {str(path.relative_to(root)):record(path) for path in windows.iterdir() if path.is_file()}
original_log = next(path for path in windows.glob('*.log'))
assert record(original_log) == '8f1841541b1aca62fba02236ea6e075f3cdd3acb8638e837d4410a1288774c61'
assert sha(subprocess.check_output(['git','show','HEAD:'+str(original_log.relative_to(root))],cwd=root)) == record(original_log)
logs = (scratch/'memory-lib-tests.log').read_text()
assert '40 passed; 0 failed; 0 ignored;' in logs
assert 'final actual SQLite timeout=3994ms' in logs
assert '980 real early BUSY attempts' in logs
assert 'actual busy callbacks=0' in logs
for path in scratch.iterdir():
    if path.is_file():
        shutil.copyfile(path,out/path.name)
shutil.copyfile(Path(__file__),out/'finalize.py')
remaining_users = users()
assert not remaining_users
(out/'no-target-users.json').write_text(json.dumps({'target':str(target),'users':remaining_users},indent=2)+'\n')
receipt = {'tested_source':head,'selected_executed_test':selected,'ELFs':elfs,'unique_sha256':len(checked),
           'all_archives_roundtrip_verified':True,'scope':'Only selected Memory library test executed; other ELF files include transferred cache/build/object/dependency files, not a claim of current executed tests.'}
(archive/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
runtime = {path.name:record(path) for path in out.iterdir() if path.is_file()}
validation = {**old,'tested_source':head,'results':{'tests_passed':40,'tests_failed':0,'tests_ignored':0,'strict_clippy':0,'fmt':0,'actionlint':0,'diffcheck':0,'outer_script_exit':0},
 'phase_proof':{**old['phase_proof'],'reserved_writer_attempts':980,'reserved_writer_exhaustion_seconds':5.000072542,'held_reader_exhaustion_seconds':5.006185954,
 'mixed_lock':{'early_busy_attempts':196,'writer_release_seconds':1.000886987,'last_actual_sqlite_timeout_ms':3994,'total_seconds':5.003546279,'row_unchanged':True,'mode':'delete','autocommit':True},
 'actual_mutant':{'source_base':'a112189','patch':'mixed-a112189/mutant.patch','actual_binary_receipt':'mixed-a112189/mutant-binary-receipt.json','exit':101,'tests_failed':1,'last_actual_sqlite_timeout_ms':5000,'total_seconds':6.01}},
 'selected_executed_test_binary':selected,'all_current_ELF_paths':len(elfs),'all_current_ELF_unique':len(checked),'elf_receipt':str(archive/'receipt.json'),
 'elf_receipt_sha256':record(archive/'receipt.json'),'source_files':sources,'corekit_source_inputs':corekit,'runtime_logs':runtime,'original_windows_files':originals,
 'prior_failures_preserved':['actual62b retained-reader callback assumption37/1','bdf PATH actionlint exit127 and explicit passed invocation','a112 genuinely compiled BUDGET-timeout mutant0/1','a837 strict Clippy unchecked test Duration subtraction after40/0; literal log and binary retained'],
 'normal_main':'5e232700bb9031fc34d4995465a4037837540abb','all_rust_cargo_byte_equal_to':'d751daadc2a7dc5cc3340953987dc95403ce8a01','target_users':0,'validation_scope':'Focused Memory only; no CLI graph or Go application/oracle replay. Go SDK version inspection only; no11434 usage.'}
(out/'validation.json').write_text(json.dumps(validation,indent=2)+'\n')
print(json.dumps({'tested_source':head,'validation':str(out/'validation.json'),'validation_sha256':record(out/'validation.json'),'selected_test':selected,'ELF_paths':len(elfs),'ELF_unique':len(checked),'target_users':0,'free_bytes':shutil.disk_usage(root).free},indent=2))
