"""Bind actual Go/native constructor, request, CLI and control evidence to source."""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys

go,native,output=map(pathlib.Path,sys.argv[1:4])
records=json.loads(go.read_text())
assert len(records)==len({row['id']for row in records})==89
expected_gates=8 if sys.platform=='win32'else 7
promoted=[row for row in records if row.get('native_promoted')]
assert len(promoted)==1 and promoted[0]['id']=='kimi-device-unicode-value' and promoted[0]['gate']=='unproven device header bytes' and not promoted[0]['gated']
assert json.loads(native.read_text())==dict(cases=89,passed=89,failed=0,full_reports=89-expected_gates,gated=expected_gates)
cli=json.loads(go.with_name('cli.json').read_text())
controls=json.loads(go.with_name('controls.json').read_text())
assert cli['cases']==cli['expected_cases']==cli['passed'] and cli['failed']==0 and cli['route_cases']==89-expected_gates
assert [item['exit']for item in controls]==[1,1,101,101,101]
owner=json.loads(go.with_name('owner.json').read_text())
owner_native=json.loads(go.with_name('owner-native.json').read_text())
owner_cli=json.loads(go.with_name('owner-cli.json').read_text())
owner_controls=json.loads(go.with_name('owner-controls.json').read_text())
paths=json.loads(go.with_name('path.json').read_text())
paths_native=json.loads(go.with_name('path-native.json').read_text())
assert len(owner)==len({row['id']for row in owner})==16
assert owner_native==dict(cases=16,passed=16,failed=0,full_reports=16,read_only=16)
assert owner_cli['cases']==owner_cli['passed']==owner_cli['route_cases']==owner_cli['read_only_source_cases']==16 and owner_cli['failed']==0
assert owner_cli['binary_sha256']==cli['binary_sha256']
assert [row['exit']for row in owner_controls]==[101]
assert len(paths)==paths_native['cases']==paths_native['passed']==(21 if sys.platform=='win32'else 22) and paths_native['failed']==0
argv=json.loads(go.with_name('argv.json').read_text())
argv_controls=json.loads(go.with_name('argv-controls.json').read_text())
argv_build=json.loads(go.with_name('argv-build.json').read_text())
assert argv['cases']==argv['passed']==(56 if sys.platform=='win32'else 82) and argv['failed']==0
assert argv['original_cases']==(14 if sys.platform=='win32'else 22) and argv['original_parent_mismatches']==(6 if sys.platform=='win32'else 12)
assert argv_controls['rejected']==(2 if sys.platform=='win32'else 3) and all(row['rejected']for row in argv_controls['controls'])
assert argv['parent_source']==argv_build['parent_source']=='abf20713bacdab562644256e27616a9dd7acb81e' and argv_build['parent_source_clean']
assert argv['binary_sha256']==argv_controls['binary_sha256']
assert argv['binary_sha256']['go']==cli['binary_sha256']['go']
assert argv['binary_sha256']['rust']==cli['binary_sha256']['rust']==argv_build['binaries_sha256']['rust']
assert argv['binary_sha256']['parent']==argv_build['binaries_sha256']['parent']
assert argv_build['candidate_restored_byte_identical'] and argv_build['candidate_source']==argv['candidate_head']
assert argv['candidate_head']==subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert all(hashlib.sha256((pathlib.Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())/name).read_bytes()).hexdigest()==digest for name,digest in argv['source_sha256'].items())

repo=pathlib.Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())
baseline_path=repo/'migration/evidence/usage-local-files-768/linux-go-baseline.json'
historical=baseline_path.read_bytes()
original_head='f6f548145ab4961e009ccea8e8242df9be7d2585'
assert historical==subprocess.check_output(['git','show',original_head+':'+baseline_path.relative_to(repo).as_posix()]),'unchanged original Go-only receipt'
for path,digest in json.loads(historical)['baseline_source_sha256'].items():
    assert hashlib.sha256((repo/path).read_bytes()).hexdigest()==digest,'unchanged original baseline source: '+path
for path in ['rust/symbrain-usage/tests/fixtures/copilot_file_token_oracle.json','rust/symbrain-usage/tests/fixtures/kimi_file_token_oracle.json']:
    assert (repo/path).read_bytes()==subprocess.check_output(['git','show',original_head+':'+path]),'unchanged frozen fixture'
original_input=json.loads(go.with_name('baseline-input.json').read_text())
fresh_baseline=json.loads(go.with_name('baseline-go.json').read_text())
routes=json.loads(go.with_name('baseline-native.json').read_text())
assert len(original_input)==fresh_baseline['cases']==fresh_baseline['read_only_cases']==97
assert len(routes)==len({row['id'] for row in routes})==31
current_input={row['id']:row for row in json.loads(go.with_name('input.json').read_text())}
current_records={row['id']:row for row in records}
route_records={row['id']:row for row in routes}
fresh_records={row['id']:row for row in fresh_baseline['records']}
assert set(fresh_records)=={row['id'] for row in original_input} and len(fresh_records)==97
mapping=[]
for row in original_input:
    case_id=row['id']
    if case_id in current_input:
        assert all(row[field]==current_input[case_id][field] for field in row),'exact retained original input'
        actual=current_records[case_id]
        scope='retained eligibility gate' if actual['gated'] else 'full native constructor/report/request comparison'
        mapping.append(dict(id=case_id,provider=row['provider'],proof=scope,needs_go_fallback=actual['gated'],fresh_go_original=True))
    else:
        mapping.append(dict(route_records[case_id],fresh_go_original=True))
assert sum(row['id']in current_input for row in original_input)==66
assert len(mapping)==len({row['id']for row in mapping})==97
baseline_accounting=dict(original_head=original_head,historical_receipt_sha256=hashlib.sha256(historical).hexdigest(),unchanged_baseline_source_sha256=json.loads(historical)['baseline_source_sha256'],original_inputs=97,retained_inputs=66,remaining_route_only=31,input_sha256=hashlib.sha256(go.with_name('baseline-input.json').read_bytes()).hexdigest(),fresh_go_sha256=hashlib.sha256(go.with_name('baseline-go.json').read_bytes()).hexdigest(),native_routes_sha256=hashlib.sha256(go.with_name('baseline-native.json').read_bytes()).hexdigest(),cases=mapping)

files=sorted((repo/'rust/symbrain-usage/src').rglob('*.rs'))+[repo/path for path in ['rust/symbrain-usage/Cargo.toml','Cargo.lock','rust/symbrain-cli/tests/mcp_cli_tests.rs','rust/symbrain-cli/src/usage_cli.rs','rust/symbrain-cli/src/lib.rs']]+sorted((repo/'scripts/usage-copilot-kimi-oracle').glob('*'))+sorted((repo/'scripts/usage-local-files-baseline').glob('*'))+[baseline_path]
files=[p for p in files if p.is_file()]
files += [repo/path for path in ['rust/symbrain-core/src/config/format.rs', 'rust/symbrain-core/src/config/set.rs', 'rust/symbrain-core/src/config/mod.rs', 'rust/symbrain-core/src/go_printable.rs']]
receipt={'schema_version':1,'oracle_commit':sys.argv[4],'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'candidate_dirty':bool(subprocess.check_output(['git','status','--porcelain'],text=True).strip()),'platform':sys.platform,'arch':platform.machine(),'go_sdk':subprocess.check_output(['go','version'],text=True).strip(),'rust_sdk':subprocess.check_output(['rustc','--version'],text=True).strip(),'cases':89,'passed':89,'failed':0,'full_report_cases':89-expected_gates,'retained_gates':expected_gates,'source_sha256':{str(p.relative_to(repo)):hashlib.sha256(p.read_bytes()).hexdigest()for p in files},'oracle_sha256':hashlib.sha256(go.read_bytes()).hexdigest(),'records':records,'argv_diagnostics':dict(actual=argv,controls=argv_controls,build=argv_build),'owner_selection':dict(go_cases=16,native=owner_native,records=owner,cli=owner_cli,negative_controls=owner_controls),'native_path_semantics':dict(records=paths,native=paths_native),'original_97_accounting':baseline_accounting,'cli':cli,'negative_controls':controls,'native_promotions':[dict(id=row['id'],historical_gate=row['gate'],current_scope='full actual constructor/report/request/raw-header/read-only comparison') for row in promoted],'native_macos_windows':'native exact-head CI required; Linux alone does not establish these targets','remaining_scope':'distinct Copilot tokens, unsafe/unreadable sources, unproven device headers, home mismatch on Windows, base/workspace/numeric expiry and automatic host Keychain remain gated; full #768 remains open'}
output.write_text(json.dumps(receipt,indent=2)+'\n')
