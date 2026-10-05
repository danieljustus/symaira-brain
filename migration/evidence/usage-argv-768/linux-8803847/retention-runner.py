from pathlib import Path
import hashlib,json,os,re,shutil,subprocess,tarfile
repo=Path('/workspace/symaira-usage768-argv');source='8803847278546f3161eaaa84d2e65bc2eec594a2';sha=lambda data:hashlib.sha256(data).hexdigest();dest=repo/'migration/evidence/usage-argv-768/linux-8803847';dest.mkdir()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==source and not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
files=[]
for pattern in ['symaira-usage768-argv-final2*','symaira-usage768-argv-final-retain.*']:
 for p in Path('/tmp').glob(pattern):
  if p.is_file():files.append(p)
  elif p.is_dir():files.extend(q for q in p.rglob('*')if q.is_file())
retention=[];external=[]
for p in sorted(set(files)):
 if p.read_bytes()[:4]==b'\x7fELF' or p.suffix=='.gz':
  external.append(dict(original=str(p),sha256=sha(p.read_bytes()),bytes=p.stat().st_size));continue
 q=dest/'raw'/p.relative_to('/tmp');q.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,q);assert p.read_bytes()==q.read_bytes();retention.append(dict(original=str(p),retained=str(q.relative_to(dest)),sha256=sha(p.read_bytes()),bytes=p.stat().st_size))
reports={};objects=[]
for name in ['local','files','hermes','reference']:
 p=Path('/tmp/symaira-usage768-argv-final2-'+name+'.json');r=json.loads(p.read_text());assert r['candidate_head']==source and not r['candidate_dirty'];objects.append(r)
 for path,digest in r['source_sha256'].items():
  assert sha((repo/path).read_bytes())==digest and (repo/path).read_bytes()==subprocess.check_output(['git','show',source+':'+path],cwd=repo)
 reports[name]=dict(path=str(p),retained='raw/'+p.name,sha256=sha(p.read_bytes()),source_entries=len(r['source_sha256']),constructor_cases=r.get('cases',r.get('constructor_request_cases')),cli=r['cli']['cases'],controls=len(r['negative_controls']),binaries_sha256=r['cli']['binary_sha256'])
assert [reports[x]['source_entries']for x in reports]==[69,61,59,55]
common=set.intersection(*(set(r['source_sha256'])for r in objects));assert len(common)==52
for path in common:assert len({r['source_sha256'][path]for r in objects})==1
local=objects[0];argv=local['argv_diagnostics'];assert argv['actual']['cases']==82 and argv['actual']['original_cases']==22 and argv['actual']['original_parent_mismatches']==12 and argv['controls']['rejected']==3 and argv['build']['candidate_source_clean']and argv['build']['candidate_restored_byte_identical']
for name in ['argv-initial-clean','argv-parent-clean','argv-candidate-clean']:
 r=json.load(open('/tmp/symaira-usage768-argv-final2-local.evidence/'+name+'.json'));assert r['roundtrip_verified']and not r['target_users']
 assert sha(Path(r['archive']).read_bytes())==r['archive_sha256']
 with tarfile.open(r['archive'],'r:gz')as bundle:
  for row in {v['sha256']:v for v in r['records']}.values():
   data=bundle.extractfile('sha256/'+row['sha256']).read();assert sha(data)==row['sha256']and len(data)==row['bytes']
log=Path('/tmp/symaira-usage768-argv-final2-tests.log').read_text();rows=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',log);totals=tuple(sum(int(row[i])for row in rows)for i in range(3));assert totals==(355,0,4)and len(rows)==33
for name,count in [('accepted-parent-fresh-cli-extra',22),('original-owner-replay',12),('extra',32)]:
 r=json.load(open('/tmp/symaira-usage768-argv-final2-'+name+'.json'));assert r['cases']==r['passed']==count and not r['failed']
archive=Path('/workspace/oracles/symaira-usage768-argv-880-binaries/receipt.json');ar=json.loads(archive.read_text());assert ar['source']==source and ar['roundtrip_verified']and ar['ordinary_test_binaries']==33
with tarfile.open(ar['archive'],'r:gz')as bundle:
 for row in ar['records']:
  data=bundle.extractfile('sha256/'+row['sha256']).read();assert sha(data)==row['sha256']and len(data)==row['bytes']
shutil.copyfile(archive,dest/'binary-archive-receipt.json');shutil.copyfile(archive.parent/'go-build.json',dest/'go-build.json')
original=repo/'migration/evidence/usage-argv-768/original-abf-review';old=json.load(open(original/'retention-manifest.json'));assert len(old['records'])==103
for row in old['records']:
 p=original/row['tracked_path']if row['tracked']else Path(row['retained']);assert sha(p.read_bytes())==row['sha256']
original_archive=json.load(open(original/'archive-receipt.json'))
with tarfile.open(original_archive['archive'],'r:gz')as bundle:
 for row in original_archive['executed_review_files']:
  data=bundle.extractfile(row['member']).read();assert sha(data)==row['sha256']and len(data)==row['bytes']
assert len(original_archive['executed_review_files'])==36
# The original failed shared-target CLI bytes are exactly the preserved abf CLI.
failed=json.load(open('/tmp/symaira-usage768-argv-presource-local.evidence/argv-build.json'))
assert set(failed['binaries_sha256'].values())=={argv['actual']['binary_sha256']['parent']}
assert all(any(row['sha256']==digest for row in original_archive['executed_review_files'])for digest in failed['binaries_sha256'].values())
assert not subprocess.check_output(['git','diff','abf20713bacdab562644256e27616a9dd7acb81e','--name-only','--','cmd','internal','guard','go.mod','go.sum','rust/symbrain-usage'],cwd=repo)
validation=dict(source=source,normal_main='e3dbda6cbb95429237d15a9b176209b7d107792c',original_head='abf20713bacdab562644256e27616a9dd7acb81e',original_source='00bcc6f51a2fbc00581ca7f6b7af0e868d168971',candidate_clean_before_evidence=True,reports=reports,common_source_entries=52,ordinary_tests=dict(passed=355,failed=0,ignored=4,summaries=33,all_four_oracles_explicitly_replayed=True),argv=dict(cases=82,original_cases=22,original_parent_mismatches=12,mutation_controls=3,parent_source=argv['build']['parent_source'],candidate_restored_byte_identical=True,source_sdk_sha256=argv['actual']['go_sdk_source_sha256']),owner=dict(full_constructors=16,cli=16,unix_path_pairs=22,wrong_owner_exit=101,original_inputs=12,additional_public_pairs=32),accepted_parent22=dict(cases=22,passed=22,go='Fresh retained frozen-Go CLI, matching all four source-bound gates',native_fallback='absent',accepted_parent_fallback='fresh frozen-Go CLI available; reflects accepted pre-admission contract'),env=dict(CARGO_TARGET_DIR=str(repo/'target'),CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',go_path='/workspace/toolchains/go1.26.7/bin',umask='022',lifecycle='python3 /tmp/symaira-subreaper.py',rust_sdk=subprocess.check_output(['/home/agent/.cargo/bin/rustc','-Vv'],text=True),go_sdk=subprocess.check_output(['/workspace/toolchains/go1.26.7/bin/go','version'],text=True).strip()),original103_36_roundtrip_verified=True,original73_11_parent112_74_retained=True,failed_build_variant_bytes_retained=True,go_production_frozen_fixtures_usage_production_unchanged=True,artifact_retention=retention,external_artifacts=external,actual_binary_archive=str(archive),actual_binary_archive_sha256=sha(archive.read_bytes()),strict=dict(clippy=True,workspace_fmt=True,included_fragments_fmt=18,actionlint=True,diff=True),scope='Focused Usage zero-local-flag grammar/raw Unix diagnostics only. No provider/HTTP/owner admission expansion. Windows56 cases/14 original/two mutation controls declared;26 raw Unix argv vectors explicitly unlaunchable in Windows Python, no native Windows/macOS runtime proof. Exact-head native3 CI and new full independent review remain required. Full768 open.')
(dest/'linux-validation.json').write_text(json.dumps(validation,indent=2)+'\n')
# Retain earlier passing immutable d066 source checkpoint and its supplemental runs.
earlier=repo/'migration/evidence/usage-argv-768/linux-d066ed4';earlier.mkdir();earlier_rows=[]
for p in sorted(Path('/tmp').glob('symaira-usage768-argv-final-*')):
 ps=[p]if p.is_file()else[p2 for p2 in p.rglob('*')if p2.is_file()]
 for q in ps:
  if q.read_bytes()[:4]==b'\x7fELF'or q.suffix=='.gz':continue
  out=earlier/q.relative_to('/tmp');out.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(q,out);earlier_rows.append(dict(original=str(q),retained=str(out.relative_to(earlier)),sha256=sha(q.read_bytes())))
(earlier/'retention-manifest.json').write_text(json.dumps(dict(source='d066ed4f8d2ec279792b988153e5e26ff360f055',records=earlier_rows,scope='Earlier successful clean full Linux checkpoint; superseded only by external-Cargo-target harness precondition correction. Original production and all actual results retained, no failed sample discarded. External archive references stay available.'),indent=2)+'\n')
print('PASS finalsource69/61/59/55 common52,355/0/4,82/22/3,owner16/16/22+12+32; retained',len(retention),'current artifacts +',len(earlier_rows),'priorclean artifacts;39 actual binarypaths roundtrip')
