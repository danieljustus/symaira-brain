from pathlib import Path
import hashlib,json,re,subprocess,tarfile
ROOT=Path('/workspace/symaira-guard770-raw-paths')
SOURCE='806ddcf8bd36c57b5b26d5148cf7015a465d289e'
sha=lambda b:hashlib.sha256(b).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()==SOURCE
assert not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)
assert subprocess.run(['git','merge-base','--is-ancestor','e3dbda6cbb95429237d15a9b176209b7d107792c',SOURCE],cwd=ROOT).returncode==0
process=json.loads(Path('/tmp/symaira-guard770-raw-final-process.json').read_text())
assert process['candidate_head']==SOURCE and not process['candidate_dirty']
manifest=process['candidate_source_sha256']|process['candidate_manifest_sha256']
for path in list((ROOT/'scripts/guard-standalone-oracle').glob('*'))+[ROOT/'.github/workflows/ci.yml',ROOT/'rust/symbrain-cli/src/guard_cli.rs',ROOT/'docs/adr/guard-raw-path-diagnostics.md']:
 if path.is_file():manifest[str(path.relative_to(ROOT))]=sha(path.read_bytes())
for name,expected in manifest.items():
 assert sha((ROOT/name).read_bytes())==expected,name
 assert sha(subprocess.check_output(['git','show',SOURCE+':'+name],cwd=ROOT))==expected,name
original=ROOT/'migration/evidence/guard-raw-paths-770/original-e876'
preserved=json.loads((original/'preservation.json').read_text())
for row in preserved['mapping']:
 assert sha((ROOT/row['retained']).read_bytes())==row['sha256'],row
archive=json.loads((original/'receipt.json').read_text())
archivepath=Path(archive['archive'])
assert sha(archivepath.read_bytes())==archive['archive_sha256']
with tarfile.open(archivepath) as tar:
 for name,details in archive['native_ELFs'].items():
  data=tar.extractfile(name).read()
  assert sha(data)==details['sha256'] and len(data)==details['bytes'],name
for name in ['Cargo.toml','Cargo.lock','rust/symbrain-audit','rust/symbrain-guard-core','guard/scripts/guard-decide-oracle/cases.json']:
 assert not subprocess.check_output(['git','diff','e8766e354b4f6b21617d4201de23a96d65ac47c7',SOURCE,'--',name],cwd=ROOT),name
assert not [n for n in subprocess.check_output(['git','diff','e8766e354b4f6b21617d4201de23a96d65ac47c7',SOURCE,'--name-only'],cwd=ROOT,text=True).splitlines() if n.endswith('.go')]
for name in process['go_source_sha256']:
 assert sha(subprocess.check_output(['git','show',process['oracle_ref']+':'+name],cwd=ROOT))==process['go_source_sha256'][name]
 assert sha((Path('/tmp/symaira-guard770-diagnostics-independent-go-source')/name).read_bytes())==process['go_source_sha256'][name]
extra=json.loads(Path('/tmp/symaira-guard770-raw-closure-extra.json').read_text())
assert (extra['total'],extra['matched'],extra['gated'],extra['failed'])==(31,27,4,0)
for suffix in ['raw-paths','raw-paths-unicode']:
 r=json.loads(Path('/tmp/symaira-guard770-raw-closure-'+suffix+'.json').read_text());assert len(r['results'])==4 and all(x['matched'] for x in r['results'])
for count,matched in [(80,80),(94,91)]:
 r=json.loads(Path(f'/tmp/symaira-guard770-raw-original{count}.json').read_text());assert r['candidate_head']==SOURCE and not r['candidate_dirty'] and (r['total'],r['matched'])==(count,matched)
raw=json.loads(Path('/tmp/symaira-guard770-raw-final-process-raw-paths.json').read_text());assert (raw['total'],raw['matched'],len(raw['controls']))==(63,63,2)
controls=json.loads(Path('/tmp/symaira-guard770-raw-final-process-controls.json').read_text());assert controls['rejected']==3
assert process['total']==124 and process['matched']==121 and len(process['remaining_native_diagnostic_states'])==3 and not process['audit_diagnostic_deviations']
assert all(c['rejected'] and not c['mutated_native']['stderr_hex'] and c['mutated_native']['stdout_hex'] for c in controls['controls']+raw['controls'])
log=Path('/tmp/symaira-guard770-raw-final-tests.log').read_text()
summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
assert sum(int(x[0]) for x in summaries)==168 and not any(int(x[1])+int(x[2]) for x in summaries)
parts=re.split(r'\n     Running ',log)[1:]
binaries={}
for part in parts:
 name=re.search(r'\(([^\n]+)\)',part.splitlines()[0]).group(1)
 p=Path(name);binaries[str(p)]=dict(sha256=sha(p.read_bytes()),bytes=p.stat().st_size,passed=int(re.search(r'test result: ok\. (\d+) passed;',part).group(1)))
cli=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard');assert sha(cli.read_bytes())==process['binaries_sha256']['rust']
binaries[str(cli)]=dict(sha256=sha(cli.read_bytes()),bytes=cli.stat().st_size)
GO=Path('/tmp/symaira-guard770-diagnostics-independent-go')
assert sha(GO.read_bytes())=='d3aac7bb745a62cd3d55a6c47c0c3d322adc8de242ee71b456375a61f73e8e39'
proofnames=[str(p) for p in Path('/tmp').glob('symaira-guard770-raw-final-*') if p.suffix in ['.json','.log'] and '-verification.' not in p.name]
proofnames +=[str(p) for p in Path('/tmp').glob('symaira-guard770-raw-closure-*') if p.suffix in ['.json','.py']]
proofnames += ['/tmp/symaira-guard770-raw-original80.json','/tmp/symaira-guard770-raw-original94.json','/tmp/symaira-guard770-raw-retained-corpora.py','/tmp/symaira-guard770-raw-finalize.py']
report=dict(validated_source=SOURCE,base_rejected_head=archive['head'],base_rejected_source=archive['source'],normally_integrated_main='e3dbda6cbb95429237d15a9b176209b7d107792c',candidate_source_sha256=manifest,source_inputs=len(manifest),frozen_go_ref=process['oracle_ref'],frozen_go_inputs=len(process['go_source_sha256']),sdk='Go1.26.7/Rust1.98.0',ordinary_tests=dict(passed=168,failed=0,ignored=0,non_doc_summaries=len(summaries),guard=63,core=105),gates=dict(current124={'matched':121,'explicit_TOML_gates':3},original80={'matched':80},original94={'matched':91,'explicit_TOML_gates':3},ordered31={'matched':27,'explicit_decoder_gates':4},raw63={'matched':63,'lossy_controls_rejected':2},original_raw4={'matched':4},original_unicode4={'matched':4},audit_filesystem6={'native_invariants':6,'legacy_go_security_divergences':3},original_controls_rejected=3,strict_clippy=True,fmt=True,actionlint=True),binaries=binaries,current_primary_go={'sha256':process['binaries_sha256']['go'],'note':'actual fresh runner Go executable was in owned temporary tree, removed by runner trap after all187 main/raw process cases; its1217 original sources and supplemental entry are bound in full process receipt'},additional_owned_go={'path':str(GO),'sha256':sha(GO.read_bytes()),'source':'/tmp/symaira-guard770-diagnostics-independent-go-source','sdk':'1.26.7'},preservation=dict(original_ELFs=len(archive['native_ELFs']),archive=str(archivepath),archive_sha256=archive['archive_sha256'],original_review_files_verified=len(preserved['mapping'])),proof_sha256={name:sha(Path(name).read_bytes()) for name in sorted(set(proofnames))},environment={'umask':'022','subreaper':'/tmp/symaira-subreaper.py','CARGO_TARGET_DIR':'/workspace/symaira-guard770-diagnostics/target','CARGO_INCREMENTAL':'0','CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0','Go_PATH':'/workspace/toolchains/go1.26.7/bin','operator_state':'not accessed; privateHOME/XDG and emptyPATH in every actual process'},limits=['independent review required; author checks are not approval','native macOS/Windows CI pending','rawUnix byte paths inapplicable to WindowsUTF16 and explicitly skipped there','inherited Windows audit-open wording remains fail-closed deviation','three selected TOML gates and broader malformed discovery/broken-output still open','compiled actual Brain reexport adapter is tested; full Brain executable not built locally','#770/#769 remain open; no complete cutover claim'])
Path('/tmp/symaira-guard770-raw-final-verification.json').write_text(json.dumps(report,indent=2)+'\n')
print({k:report[k] for k in ['validated_source','source_inputs','frozen_go_inputs','ordinary_tests','preservation']})
