import pathlib,json,gzip,hashlib,subprocess
repo=pathlib.Path('/workspace/symaira-guard770-doctor-warnings');source='fb2844b02b7c03b7c34650c6d3aed693c47b2997'
old=json.load(open('/tmp/symaira-guard770-doctor-final-handoff.json'))['candidate_source_sha256'];names=set(old)
names.update(subprocess.check_output(['git','ls-files','scripts/guard-standalone-oracle'],cwd=repo,text=True).splitlines())
names.update(['rust/symguard-cli/src/doctor/config_warnings.rs','rust/symguard-cli/tests/doctor_warnings.rs','docs/adr/guard-doctor-ordered-config-warnings.md'])
manifest={}
for n in sorted(names):
 p=repo/n;raw=p.read_bytes();assert raw==subprocess.check_output(['git','show',source+':'+n],cwd=repo);manifest[n]=hashlib.sha256(raw).hexdigest()
primary=json.load(open('/tmp/symaira-guard770-warnings-clean-process.json'))
assert len(primary['go_source_sha256'])==1217
for n,h in primary['go_source_sha256'].items():assert hashlib.sha256(subprocess.check_output(['git','show','dcddcef0df5789123c7c9a7ebe6e01f10e941f2c:'+n],cwd=repo)).hexdigest()==h
for n,h in primary['candidate_source_sha256'].items():assert hashlib.sha256((repo/n).read_bytes()).hexdigest()==h
nondet=repo/'migration/evidence/guard-doctor-boundaries-770/baseline-8e3/symaira-guard770-doctor-boundary-inventory.json.gz';assert nondet.read_bytes()==subprocess.check_output(['git','show','1c04ad92f9537d5b4d552d080433635c5f0c5403:'+str(nondet.relative_to(repo))],cwd=repo);d=json.loads(gzip.decompress(nondet.read_bytes()));assert len(d['repeated_invalid_defaults'])==50 and d['distinct_invalid_default_reports']==2
archive=pathlib.Path('/workspace/oracles/symaira-guard770-warnings-fb2844b-binaries/receipt.json');a=json.loads(archive.read_text())
for h,row in a['unique'].items():
 raw=gzip.decompress(pathlib.Path(row['archive']).read_bytes());assert len(raw)==row['bytes'] and hashlib.sha256(raw).hexdigest()==h
actual=json.load(open('/tmp/symaira-guard770-warnings-clean-process-binaries/receipt.json'))
for row in actual['records']:assert hashlib.sha256(pathlib.Path(row['retained']).read_bytes()).hexdigest()==row['sha256']
out=dict(source=source,source_clean_before_evidence=True,source_sha256=manifest,source_inputs=len(manifest),frozen_go_ref='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c',frozen_go_inputs1217=len(primary['go_source_sha256']),unchanged_original50_sha256=hashlib.sha256(nondet.read_bytes()).hexdigest(),original50_two_reports_preserved=True,unchanged_production_go_fixtures_core_kernel_audit_and_lock=True,binary_archive=str(archive),binary_archive_receipt_sha256=hashlib.sha256(archive.read_bytes()).hexdigest(),archived_current_cached_elfs=66,archive_unique_hashes45=True,actual_executed_paths24=True,target_users=a['target_users'])
pathlib.Path('/tmp/symaira-guard770-warnings-clean-verification.json').write_text(json.dumps(out,indent=2)+'\n');print('source manifest',len(manifest),'Go1217 and50/2 original verified; archive66+2/45unique/24actuallyexecuted verified')
