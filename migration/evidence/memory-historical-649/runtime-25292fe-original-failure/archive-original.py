from pathlib import Path
import datetime, gzip, hashlib, json, re, shutil, subprocess

root = Path('/workspace/symaira-memory649-integrated')
target = root/'target'
report = Path('/tmp/symaira-memory649-runtime-25292fe')
archive = Path('/workspace/oracles/symaira-memory649-25292fe-failure-binaries')
archive.mkdir(exist_ok=True)
def sha(raw):
    return hashlib.sha256(raw).hexdigest()
def users():
    found=[]
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        for link in [proc/'exe',proc/'cwd',*list((proc/'fd').glob('*'))]:
            try:
                value=str(link.resolve(strict=True))
            except (OSError,RuntimeError):
                continue
            if value==str(target) or value.startswith(str(target)+'/'):
                found.append(dict(pid=proc.name,link=str(link),value=value))
    return found
assert not users()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()=='25292fe2d3349fe3b716783310096accb1f63007'
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
log=(report/'all-target-tests.log').read_text()
executed=[]
for match in re.finditer(r'Running [^\n]* \(([^\n]+)\)',log):
    path=Path(match.group(1))
    if not path.is_absolute():
        path=root/path
    executed.append(dict(path=str(path.resolve()),sha256=sha(path.read_bytes())))
payloads={}
elfs=[]
for path in sorted(target.rglob('*')):
    if not path.is_file():
        continue
    with path.open('rb') as stream:
        if stream.read(4)!=b'\x7fELF':
            continue
    raw=path.read_bytes();digest=sha(raw)
    if digest not in payloads:
        gz=archive/(digest+'.gz');compressed=gzip.compress(raw,compresslevel=9,mtime=0);gz.write_bytes(compressed)
        assert sha(gzip.decompress(gz.read_bytes()))==digest
        payloads[digest]=dict(gzip=str(gz),gzip_sha256=sha(compressed),bytes=len(raw))
    elfs.append(dict(path=str(path),sha256=digest,archive=payloads[digest]))
for item in executed:
    assert item['sha256'] in payloads
    item['archive']=payloads[item['sha256']]
cli=target/'debug/symbrain'
cli_record=dict(path=str(cli),sha256=sha(cli.read_bytes()),archive=payloads[sha(cli.read_bytes())],
                meaning='Cargo built CLI used by earlier native process tests; explicit later production build stage was not reached')
summaries=re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
receipt=dict(source='25292fe2d3349fe3b716783310096accb1f63007',outer_driver_exit=1,cargo_stage_exit=101,
 tests=dict(passed=sum(int(x[1]) for x in summaries),failed=sum(int(x[2]) for x in summaries),ignored=sum(int(x[3]) for x in summaries),summaries=len(summaries),
 memory_library=dict(passed=61,failed=4,ignored=0)),actual_executed_roles=executed,cli=cli_record,
 ELF_files=elfs,unique_payloads=len(payloads),all_archives_roundtrip_verified=True,
 scope='All failures and actual executed bytes retained before any test/source correction; unknown binary/artifact roles not claimed as executed.',
 original_full_driver_validation_sha256=sha((report/'validation.json').read_bytes()),
 original_reports={str(p.relative_to(report)):sha(p.read_bytes()) for p in report.rglob('*') if p.is_file()},
 target_users=users(),timestamp=datetime.datetime.now(datetime.timezone.utc).isoformat(),free_bytes=shutil.disk_usage(root).free)
assert not receipt['target_users']
(archive/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
out=root/'migration/evidence/memory-historical-649/runtime-25292fe-original-failure'
out.mkdir()
for path in report.iterdir():
    if path.is_file():
        shutil.copyfile(path,out/path.name)
prep=Path('/tmp/symaira-memory649-runtime-25292fe-prep')
for path in prep.iterdir():
    if path.is_file():
        shutil.copyfile(path,out/('prep-'+path.name))
shutil.copyfile(Path(__file__),out/'archive-original.py')
(out/'failure-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(dict(source=receipt['source'],tests=receipt['tests'],actual_roles=len(executed),ELF_files=len(elfs),unique=len(payloads),CLI_SHA=cli_record['sha256'],archive_receipt=str(archive/'receipt.json'),target_users=0,free_bytes=receipt['free_bytes']),indent=2))
