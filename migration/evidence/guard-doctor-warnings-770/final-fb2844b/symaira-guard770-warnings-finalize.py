import gzip,hashlib,json,pathlib,re,subprocess,shutil
repo=pathlib.Path('/workspace/symaira-guard770-doctor-warnings');source='fb2844b02b7c03b7c34650c6d3aed693c47b2997'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==source
assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
verify=json.load(open('/tmp/symaira-guard770-warnings-clean-verification.json'))
for name,digest in verify['source_sha256'].items():assert hashlib.sha256((repo/name).read_bytes()).hexdigest()==digest
prefix='/tmp/symaira-guard770-warnings-clean-'
reports={name:json.load(open(prefix+name+'.json'))for name in ['process','process-controls','process-raw-paths','process-doctor-boundaries','process-config-warnings','original80','original94','parent-mixed35','closure-extra','closure-audit-filesystem','closure-raw-paths','closure-raw-paths-unicode','parent-new32']}
for name,r in reports.items():
 assert r.get('candidate_head',r.get('head'))==source,(name,r.get('candidate_head',r.get('head')))
 assert not r.get('candidate_dirty',False)and r.get('candidate_clean',True),name
 if 'failed'in r:assert r['failed']==0,name
 if 'results'in r:
  assert all(not row.get('disposition','').startswith('failed')for row in r['results']),name
assert (reports['process']['total'],reports['process']['matched'])==(124,122)
assert len(reports['process']['remaining_native_diagnostic_states'])==2
assert (reports['process-doctor-boundaries']['total'],reports['process-doctor-boundaries']['matched'],reports['process-doctor-boundaries']['gated'])==(78,67,11)
warning=reports['process-config-warnings'];assert(warning['total'],warning['matched'],warning['gated'])==(54,46,8)
assert len(warning['controls'])==5 and warning['repeated_distinct_stderr']==1 and len(warning['repeated_go_runs'])==10
assert reports['original80']['matched']==80 and reports['original94']['matched']==92
assert reports['closure-extra']['matched']==27 and reports['closure-extra']['gated']==4
assert reports['parent-mixed35']['matched']==35 and reports['parent-new32']['matched']==32
assert len(reports['closure-audit-filesystem']['results'])==6 and all(row['native_invariant_passed']for row in reports['closure-audit-filesystem']['results'])
for name in ['closure-raw-paths','closure-raw-paths-unicode']:assert len(reports[name]['results'])==4 and all(row['matched']for row in reports[name]['results'])
ordinary=[];kernel=[]
for name,target in [('tests',ordinary),('kernel-tests',kernel)]:
 text=pathlib.Path(prefix+name+'.log').read_text();target.extend(tuple(map(int,m))for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',text))
assert len(ordinary)==14 and tuple(sum(row[i]for row in ordinary)for i in range(3))==(173,0,0)
assert len(kernel)==7 and tuple(sum(row[i]for row in kernel)for i in range(3))==(41,0,0)
current=json.load(open(prefix+'process-binaries/receipt.json'));assert current['candidate_head']==source and current['candidate_dirty']is False
binary={row['role']:row for row in current['records']}
for row in current['records']:assert hashlib.sha256(pathlib.Path(row['retained']).read_bytes()).hexdigest()==row['sha256']
for name,r in reports.items():
 for key in ['binaries_sha256','binary_sha256','binaries']:
  if key not in r:continue
  for label,digest in r[key].items():
   if label in ['go','native','rust']:role='go'if label=='go'else'native'
   else:role='go'if label.endswith('/go')else'native'
   assert digest==binary[role]['sha256'],(name,label,digest,binary[role]['sha256'])
# Copy only proof objects after all clean-source executions/verification finish.
checkpoint=repo/'migration/evidence/guard-doctor-warnings-770/checkpoint-707e609'
shutil.copytree('/tmp/symaira-guard770-warnings-checkpoint707-tracked-proofs',checkpoint)
dirty=repo/'migration/evidence/guard-doctor-warnings-770/supplemental-evidence-dirty-attempt';dirty.mkdir()
for p in sorted(pathlib.Path('/tmp/symaira-guard770-warnings-supplemental-dirty-attempt').iterdir()):
 raw=p.read_bytes();q=dirty/(p.name+('.gz'if p.suffix in ['.json','.log']else''));q.write_bytes(gzip.compress(raw,mtime=0)if q.suffix=='.gz'else raw);assert(gzip.decompress(q.read_bytes())if q.suffix=='.gz'else q.read_bytes())==raw
final=repo/'migration/evidence/guard-doctor-warnings-770/final-fb2844b';final.mkdir()
proofs=[p for p in sorted(pathlib.Path('/tmp').glob('symaira-guard770-warnings-clean-*'))if p.is_file() and p.suffix in ['.json','.py','.log']]
proofs += [pathlib.Path('/tmp/symaira-guard770-warnings-verification-first-failure.log'),pathlib.Path('/tmp/symaira-guard770-warnings-first-commit-check-failure.log'),pathlib.Path('/tmp/symaira-guard770-warnings-finalize.py')]
retention=[]
for p in proofs:
 raw=p.read_bytes();q=final/(p.name+('.gz'if p.suffix in ['.json','.log']else''));q.write_bytes(gzip.compress(raw,mtime=0)if q.suffix=='.gz'else raw);assert(gzip.decompress(q.read_bytes())if q.suffix=='.gz'else q.read_bytes())==raw
 retention.append(dict(original=str(p),retained=str(q.relative_to(repo)),sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw)))
validation=dict(validated_source=source,approved_parent='1c04ad92f9537d5b4d552d080433635c5f0c5403',normal_main_ancestor='e3dbda6cbb95429237d15a9b176209b7d107792c',candidate_clean_before_evidence=True,candidate_source_sha256=verify['source_sha256'],source_inputs=105,frozen_go_ref=verify['frozen_go_ref'],frozen_go_inputs=1217,ordinary_tests=dict(passed=173,failed=0,ignored=0,summaries=14,baseline170_plus2metadata_plus1shared_consumer=True,consumer_actual_child_roles=['healthy','semantic','type','discovery','rawUnix']),separate_kernel=dict(passed=41,failed=0,ignored=0,summaries=7,overlaps_ordinary_kernel_tests=True),gates=dict(current124=dict(matched=122,remaining_toml=2),additive78=dict(matched=67,remaining_toml=11),raw63=dict(matched=63),warning54=dict(matched=46,remaining=8,original_inputs14=True,go_repeats=10,distinct_go_warning_order=1,windows_declared_launchable=52,unix_inapplicable_windows=2),original80=dict(matched=80),original94=dict(matched=92,remaining=2),ordered31=dict(matched=27,gated=4),mixed35=dict(matched=35),original_raw4=dict(matched=4),original_unicode4=dict(matched=4),audit6=dict(native_invariants=6,not_blanket_go_equality=True),inherited_root32=dict(matched=32),actual_controls=dict(original_diagnostic=3,original_raw=2,original_additive=3,new_warning=5,total=13)),strict=dict(all_target_all_feature_clippy=True,standalone_kernel_all_target_clippy=True,workspace_fmt=True,actionlint=True),nondeterminism=dict(original_go_runs=50,distinct_go_reports=2,byte_identical_to_approved_parent=True,sha256=verify['unchanged_original50_sha256'],exception_added=False),actual_binaries=binary,archive=dict(receipt=verify['binary_archive'],sha256=verify['binary_archive_receipt_sha256'],all_cached_elf_paths=66,additional_copied_gate_binaries=2,unique_hashes=45,actually_executed_paths=24,roundtrip_verified=True,inactive_cache_not_counted_as_current_executed=True),retention=retention,environment=dict(umask='022',subreaper='/tmp/symaira-subreaper.py',CARGO_TARGET_DIR='/workspace/symaira-guard770-diagnostics/target',CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',jobs=2,go_path='/workspace/toolchains/go1.26.7/bin',rust=subprocess.check_output(['/home/agent/.cargo/bin/rustc','-Vv'],text=True),go=subprocess.check_output(['/workspace/toolchains/go1.26.7/bin/go','version'],text=True).strip()),old_root36_and_parent65_43_archives_preserved=True,production_go_fixtures_core_kernel_audit_lock_unchanged=True,limits=['Author validation is not approval; full different-author review required','Exact-head native macOS/Windows/protected checks pending','Known case-fold aliases, ordered TOML decoder, malformed discovery and filesystem/output boundaries remain Go-owned','Original50 nondeterminism has no exception/reclassification','Brain adapter child execution is not whole Brain executable acceptance','No full770/769 closure or installed Go removal','No new dependency/capability/kernel/audit authority or GitHub writes'])
(final/'validation.json').write_text(json.dumps(validation,indent=2)+'\n');(final/'README.md').write_text('Clean source fb2844b actual source-bound Linux checkpoint. All105 candidate/1217 frozen Go inputs, actual binaries and original raw failures are retained; see validation.json. Native3/protected/different-author independent review and full770/769 remain required.\n')
print('Final proof objects',len(retention),'source105/Go1217;173ordinary +41overlapping kernel;13actualcontrols; ready evidence-only commit')
