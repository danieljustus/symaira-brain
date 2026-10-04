from pathlib import Path
import gzip,hashlib,json,re,subprocess,tarfile
ROOT=Path('/workspace/symaira-guard770-doctor-boundaries');SOURCE='9603d24e6749ee67e5eac1b7ddda6900af022cd5';PARENT='8e3b21b4fa649847f5b9a26a80f0c24fa172e1a0'
sha=lambda data:hashlib.sha256(data).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()==SOURCE
assert not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)
process=json.loads(Path('/tmp/symaira-guard770-doctor-final-process.json').read_text());assert process['candidate_head']==SOURCE and not process['candidate_dirty']
manifest=process['candidate_source_sha256']|process['candidate_manifest_sha256']
for p in list((ROOT/'scripts/guard-standalone-oracle').glob('*'))+[ROOT/name for name in ['.github/workflows/ci.yml','rust/symbrain-cli/src/guard_cli.rs','docs/adr/guard-doctor-equivalent-config-and-anchor-unicode.md','migration/guard-doctor-boundary-inventory-770.md','migration/implementation-plan.md','migration/contract-matrix.csv']]:
 if p.is_file():manifest[str(p.relative_to(ROOT))]=sha(p.read_bytes())
for name,digest in manifest.items():
 assert sha((ROOT/name).read_bytes())==digest and sha(subprocess.check_output(['git','show',SOURCE+':'+name],cwd=ROOT))==digest,name
for name in ['Cargo.toml','Cargo.lock','rust/symbrain-core','rust/symbrain-audit','guard/scripts/guard-decide-oracle/cases.json','rust/symbrain-guard-core/tests','rust/symguard-cli/src/lib.rs','rust/symguard-cli/src/audit_error.rs','rust/symguard-cli/src/guard_doctor.rs']:
 assert not subprocess.check_output(['git','diff',PARENT,SOURCE,'--',name],cwd=ROOT),name
assert subprocess.check_output(['git','diff',PARENT,SOURCE,'--name-only','--','rust/symbrain-guard-core'],cwd=ROOT,text=True).strip()=='rust/symbrain-guard-core/src/go_json.rs'
assert not [n for n in subprocess.check_output(['git','diff',PARENT,SOURCE,'--name-only'],cwd=ROOT,text=True).splitlines() if n.endswith('.go')]
for name,digest in process['go_source_sha256'].items():
 assert sha(subprocess.check_output(['git','show',process['oracle_ref']+':'+name],cwd=ROOT))==digest,name
 assert sha((Path('/tmp/symaira-guard770-diagnostics-independent-go-source')/name).read_bytes())==digest,name
assert subprocess.run(['git','merge-base','--is-ancestor','e3dbda6cbb95429237d15a9b176209b7d107792c',SOURCE],cwd=ROOT).returncode==0
retained=json.loads((ROOT/'migration/evidence/guard-raw-paths-770/independent-806/retention.json').read_text())
for name,r in retained.items():
 data=(ROOT/name).read_bytes();assert sha(data)==r['tracked_sha256'];raw=gzip.decompress(data) if name.endswith('.gz') else data;assert sha(raw)==r['original_sha256'] and len(raw)==r['original_bytes']
archive=json.loads((ROOT/'migration/evidence/guard-doctor-boundaries-770/baseline-8e3/binary-archive-receipt.json').read_text());assert sha(Path(archive['archive']).read_bytes())==archive['archive_sha256']
with tarfile.open(archive['archive']) as tar:
 for name,details in archive['ELFs'].items():
  raw=tar.extractfile(name).read();assert sha(raw)==details['sha256'] and len(raw)==details['bytes']
with gzip.open(ROOT/'migration/evidence/guard-doctor-boundaries-770/baseline-8e3/symaira-guard770-doctor-boundary-inventory.json.gz','rt') as stream:baseline=json.load(stream)
assert len(baseline['repeated_invalid_defaults'])==50 and baseline['distinct_invalid_default_reports']==2
assert baseline['candidate_head']==PARENT and not baseline['candidate_dirty']
additive=json.loads(Path('/tmp/symaira-guard770-doctor-final-process-doctor-boundaries.json').read_text());assert (additive['total'],additive['matched'],additive['gated'],len(additive['controls']))==(78,65,13,3)
raw=json.loads(Path('/tmp/symaira-guard770-doctor-final-process-raw-paths.json').read_text());assert (raw['total'],raw['matched'],len(raw['controls']))==(63,63,2)
original_controls=json.loads(Path('/tmp/symaira-guard770-doctor-final-process-controls.json').read_text());assert original_controls['rejected']==3
assert (process['total'],process['matched'],len(process['remaining_native_diagnostic_states']))==(124,121,3)
assert not process['audit_diagnostic_deviations']
for report in [additive,raw,original_controls]:assert report['candidate_head']==SOURCE and not report['candidate_dirty']
for control in additive['controls']+raw['controls']+original_controls['controls']:
 assert control['rejected']
 mutation=control['mutated_native']
 if control['id']=='gate-repaired-anchor':
  assert mutation['exit_code']==1 and not mutation['stdout_hex'] and bytes.fromhex(mutation['stderr_hex'])==b'symguard doctor: unsupported native diagnostic state; no legacy fallback is available\n'
 else:assert mutation['stdout_hex'] and not mutation['stderr_hex']
for count,matched in [(80,80),(94,91)]:
 r=json.loads(Path(f'/tmp/symaira-guard770-doctor-original{count}.json').read_text());assert r['candidate_head']==SOURCE and not r['candidate_dirty'] and (r['total'],r['matched'])==(count,matched)
ordered=json.loads(Path('/tmp/symaira-guard770-doctor-closure-extra.json').read_text());assert (ordered['total'],ordered['matched'],ordered['gated'],ordered['failed'])==(31,27,4,0)
for suffix in ['raw-paths','raw-paths-unicode']:
 r=json.loads(Path('/tmp/symaira-guard770-doctor-closure-'+suffix+'.json').read_text());assert r['head']==SOURCE and len(r['results'])==4 and all(x['matched'] for x in r['results'])
mixed=json.loads(Path('/tmp/symaira-guard770-doctor-parent-mixed35.json').read_text());assert mixed['candidate_head']==SOURCE and mixed['candidate_clean'] and (mixed['total'],mixed['matched'])==(35,35)
audit=json.loads(Path('/tmp/symaira-guard770-doctor-closure-audit-filesystem.json').read_text());assert audit['head']==SOURCE and audit['cases']==6 and all(x['native_invariant_passed'] for x in audit['results'])
binaries={}
for logname,expected in [('final-tests.log',170),('final-kernel-tests.log',41)]:
 log=Path('/tmp/symaira-guard770-doctor-'+logname).read_text();summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
 assert sum(int(x[0]) for x in summaries)==expected and not any(int(x[1])+int(x[2]) for x in summaries)
 for part in re.split(r'\n     Running ',log)[1:]:
  name=re.search(r'\(([^\n]+)\)',part.splitlines()[0]).group(1);p=Path(name)
  binaries[name]=dict(sha256=sha(p.read_bytes()),bytes=p.stat().st_size,passed=int(re.search(r'test result: ok\. (\d+) passed;',part).group(1)))
cli=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard');assert sha(cli.read_bytes())==process['binaries_sha256']['rust'];binaries[str(cli)]=dict(sha256=sha(cli.read_bytes()),bytes=cli.stat().st_size)
GO=Path('/tmp/symaira-guard770-diagnostics-independent-go');assert sha(GO.read_bytes())=='d3aac7bb745a62cd3d55a6c47c0c3d322adc8de242ee71b456375a61f73e8e39'
proofs=[p for p in Path('/tmp').glob('symaira-guard770-doctor-final-*') if p.suffix in ['.json','.log'] and '-verification.' not in p.name]
proofs += [p for p in Path('/tmp').glob('symaira-guard770-doctor-closure-*') if p.suffix in ['.json','.py']]
proofs += [Path('/tmp')/n for n in ['symaira-guard770-doctor-original80.json','symaira-guard770-doctor-original94.json','symaira-guard770-doctor-retained-corpora.py','symaira-guard770-doctor-parent-mixed35.py','symaira-guard770-doctor-parent-mixed35.json','symaira-guard770-doctor-finalize.py']]
report=dict(validated_source=SOURCE,approved_parent=PARENT,main='e3dbda6cbb95429237d15a9b176209b7d107792c',candidate_source_sha256=manifest,source_inputs=len(manifest),frozen_go_ref=process['oracle_ref'],frozen_go_inputs=len(process['go_source_sha256']),SDK='Go1.26.7/Rust1.98.0',ordinary_tests={'passed':170,'failed':0,'ignored':0,'summaries':13,'core':105,'guard':65,'independent_kernel_feature_graph':41},gates={'current124':{'matched':121,'TOML_gates':3},'original80':{'matched':80},'original94':{'matched':91,'TOML_gates':3},'raw63':{'matched':63},'additive78':{'matched':65,'explicit_TOML_gates':13},'ordered31':{'matched':27,'decoder_gates':4},'mixed35':{'matched':35},'original_raw4':{'matched':4},'original_unicode4':{'matched':4},'audit6':{'native_invariants':6,'legacy_Go_safety_differences':3},'actual_controls_rejected':8,'strict_all_target_all_feature_clippy':True,'fmt':True,'actionlint':True},baseline_positive_forms_closed={'TOML':9,'anchor_unicode':5},nondeterminism={'original_runs_preserved':50,'actual_distinct_Go_reports':2,'exception_added':False},binaries=binaries,primary_fresh_Go={'sha256':process['binaries_sha256']['go'],'note':'owned temporary CLI removed by run.sh trap; actual1217 sources/supplemental entry/SKD retained in process report'},additional_owned_Go={'path':str(GO),'sha256':sha(GO.read_bytes()),'source':'/tmp/symaira-guard770-diagnostics-independent-go-source','SDK':'1.26.7'},preservation={'current_ELFs':len(archive['ELFs']),'required_current14_verified':len(archive['required_current14']),'archive':archive['archive'],'archive_sha256':archive['archive_sha256'],'root_proof_objects_roundtrip_verified':len(retained),'older53_and_original_failures':'unchanged inherited archives and tracked receipts'},proof_sha256={str(p):sha(p.read_bytes()) for p in sorted(set(proofs))},environment={'umask':'022','subreaper':'/tmp/symaira-subreaper.py','CARGO_TARGET_DIR':'/workspace/symaira-guard770-diagnostics/target','CARGO_INCREMENTAL':'0','CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0','Go_PATH':'/workspace/toolchains/go1.26.7/bin','private_HOME_XDG_empty_PATH':True},limits=['author checks are not independent approval','native macOS/Windows acceptance pending','three original TOML gates remain, including warning-only Go healthy states','ordered decoder/malformed-discovery/read/stat/output boundaries remain explicit','no exception for nondeterministic Go invalid defaults','no Go/fixture/lock/crypto/policy/audit authority changes','no complete #770/#769 or full Brain cutover claim'])
Path('/tmp/symaira-guard770-doctor-final-verification.json').write_text(json.dumps(report,indent=2)+'\n');print({k:report[k] for k in ['validated_source','source_inputs','frozen_go_inputs','ordinary_tests','gates','preservation']})
