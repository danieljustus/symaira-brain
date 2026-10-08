import gzip, hashlib, json, pathlib, re, subprocess

repo = pathlib.Path('/workspace/symaira-memory758-sigpipe')
out = repo / 'migration/evidence/memory-cli-758/sigpipe-fix-458e8fa'
out.mkdir(parents=True, exist_ok=True)
source = '458e8fa5dc752ee2eed6446cc279649ad3d2b879'
main = 'e3dbda6cbb95429237d15a9b176209b7d107792c'
target = pathlib.Path('/workspace/symaira-memory758-writes/target')
frozen = pathlib.Path('/workspace/oracles/daemon772-go-source')
gate = pathlib.Path('/tmp/symaira-memory758-sigpipe-458-write-gate')
digest = lambda b: hashlib.sha256(b).hexdigest()
load = lambda p: json.loads(pathlib.Path(p).read_text())
run = lambda args: subprocess.check_output(args, cwd=repo).decode().strip()
assert run(['git','rev-parse','HEAD']) == source
assert run(['git','status','--porcelain']) == ''
subprocess.run(['git','merge-base','--is-ancestor',main,source],cwd=repo,check=True)
receipt = load(gate/'receipt.json')
assert receipt['candidate_revision']==source and receipt['candidate_dirty'] is False
for k, root in [('candidate_source_sha256',repo),('frozen_go_source_sha256',frozen)]:
    for name, expected in receipt[k].items():
        assert digest((root/name).read_bytes())==expected,(k,name)
for name, expected in receipt['report_sha256'].items():
    assert digest((gate/name).read_bytes())==expected,name
for name,total in [('writes',60),('deletes',16),('fallback-boundaries',13),('write-failures',10)]:
    d=load(gate/(name+'.json')); assert d['passed']==d['total']==total
assert len(receipt['controls'])==3 and all(x['detected'] and x['exit']==1 for x in receipt['controls'])
read=load('/tmp/symaira-memory758-sigpipe-458-read.json')
assert read['candidate_revision']==source and not read['candidate_dirty']
assert read['passed']==read['cases']==590
assert read['candidate_source_sha256']==receipt['candidate_source_sha256']
for name, expected in read['immutable_go_cli_source_sha256'].items():
    assert digest((frozen/name).read_bytes())==expected,name
controls=load('/tmp/symaira-memory758-sigpipe-458-read-controls.json')['controls']
assert len(controls)==2 and all(x['exit']==1 and x['cases']==x['rejected_cases']==32 for x in controls)
original=load('/tmp/symaira-memory758-sigpipe-458-original-closed-pipe.json')
assert original['head']==source and len(original['records'])==2
for record in original['records']:
    assert record['match'] and record['committed_state_matches']
    for domain in ['go','rust']:
        result=record[domain]; assert result['transcript']['exit']==-13 and result['transcript']['stderr']==''
        assert result['fts']['ok']
counts={}; binary_paths=[target/'debug/symbrain']
for n in ['cli-tests','memory-tests','activity-tests','sigpipe-child']:
    text=pathlib.Path('/tmp/symaira-memory758-sigpipe-458-'+n+'.log').read_text()
    matches=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)
    assert matches and all(int(f)==int(i)==0 for p,f,i in matches)
    counts[n]=[int(p) for p,f,i in matches]
    binary_paths += [pathlib.Path(x) for x in re.findall(r'Running .*?\((.*?)\)',text)]
assert sum(sum(v) for v in counts.values())==165
binaries={str(p):digest(p.read_bytes()) for p in binary_paths}
assert binaries[str(target/'debug/symbrain')]==receipt['rust_binary_sha256']==read['rust_binary_sha256']
archive=load('/workspace/oracles/symaira-memory758-647-binaries/receipt.json')
assert len(archive['files'])==6
for item in archive['files'].values():
    compressed=pathlib.Path(item['archive']).read_bytes()
    assert digest(compressed)==item['archive_sha256']
    original_bytes=gzip.decompress(compressed)
    assert len(original_bytes)==item['original_bytes'] and digest(original_bytes)==item['original_sha256']
files=list(gate.iterdir())+list(pathlib.Path('/tmp').glob('symaira-memory758-sigpipe-458-*'))
files += list(pathlib.Path('/tmp').glob('symaira-memory758-sigpipe-initial-*.log'))
retained={}
for p in sorted(set(files)):
    if not p.is_file() or p.name.endswith('-retain.py'): continue
    data=p.read_bytes(); name=p.name
    stored=gzip.compress(data,mtime=0) if p.suffix=='.json' else data
    q=out/(name+'.gz' if p.suffix=='.json' else name)
    q.write_bytes(stored)
    assert (gzip.decompress(q.read_bytes()) if p.suffix=='.json' else q.read_bytes())==data
    retained[str(q.relative_to(repo))]=dict(original_path=str(p),original_sha256=digest(data),original_bytes=len(data),tracked_sha256=digest(stored))
production={}
for name in run(['git','diff','--name-only','55e3038',source,'--','rust']).splitlines():
    if '/src/' in name and name.endswith('.rs'):
        production[name]=len((repo/name).read_text().splitlines());assert production[name]<400,name
sdk={c:run(c.split()) for c in ['rustc -Vv','cargo -V','/workspace/toolchains/go1.26.7/bin/go version']}
verification=dict(validated_clean_source=source,integrated_main=main,
    original_reviewed_source=archive['source'],original_publication=archive['publication'],
    gates=dict(set_pairs=60,delete_pairs=16,actual_go_boundaries=13,governance_callback_pairs=6,actual_unix_full_device_pairs=2,actual_unix_closed_reader_pairs=2,
        original_independent_closed_reader_pairs=2,write_controls=3,baseline_pairs=590,baseline_controls=2,rust_test_counts=counts,rust_total=165,strict_clippy=True,fmt=True,actionlint=True),
    retained=retained,binary_sha256=binaries,production_lines=production,sdk=sdk,
    candidate_source_hashes_verified=len(receipt['candidate_source_sha256']),frozen_go_hashes_verified=len(receipt['frozen_go_source_sha256']),
    actual_sdk_sha256=digest(pathlib.Path('/workspace/toolchains/go1.26.7/bin/go').read_bytes()),
    actual_go_cli_sha256=digest(pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0').read_bytes()),
    previous_six_executable_archives_decompression_verified=True,native_windows_macos_runtime_verified=False,
    independent_corrected_review_pending=True,full_758_pending=True,operator_state_used=False,paid_endpoints_used=False)
(out/'verification.json').write_text(json.dumps(verification,indent=2)+'\n')
print(json.dumps({'source':source,'retained_artifacts':len(retained),'rust_total':165,'cli_sha256':receipt['rust_binary_sha256'],'candidate_hashes':163,'frozen_hashes':1215,'actual_binary_count':len(binaries)},indent=2))
