"""Bind actual Go/native constructor, request, CLI and control evidence to source."""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys

go,native,output=map(pathlib.Path,sys.argv[1:4])
records=json.loads(go.read_text())
assert len(records)==len({row['id']for row in records})==86
expected_gates=6 if sys.platform=='win32'else 5
assert json.loads(native.read_text())==dict(cases=86,passed=86,failed=0,full_reports=86-expected_gates,gated=expected_gates)
cli=json.loads(go.with_name('cli.json').read_text())
controls=json.loads(go.with_name('controls.json').read_text())
assert cli['cases']==cli['expected_cases']==cli['passed'] and cli['failed']==0 and cli['route_cases']==86-expected_gates
assert [item['exit']for item in controls]==[1,1,101,101,101]
repo=pathlib.Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())
files=sorted((repo/'rust/symbrain-usage/src').rglob('*.rs'))+[repo/path for path in ['rust/symbrain-usage/Cargo.toml','Cargo.lock','rust/symbrain-cli/tests/mcp_cli_tests.rs','rust/symbrain-cli/src/usage_cli.rs','rust/symbrain-cli/src/lib.rs']]+sorted((repo/'scripts/usage-copilot-kimi-oracle').glob('*'))+[repo/'scripts/usage-local-files-baseline/cases.py']
files=[p for p in files if p.is_file()]
receipt={'schema_version':1,'oracle_commit':sys.argv[4],'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'candidate_dirty':bool(subprocess.check_output(['git','status','--porcelain'],text=True).strip()),'platform':sys.platform,'arch':platform.machine(),'go_sdk':subprocess.check_output(['go','version'],text=True).strip(),'rust_sdk':subprocess.check_output(['rustc','--version'],text=True).strip(),'cases':86,'passed':86,'failed':0,'full_report_cases':86-expected_gates,'retained_gates':expected_gates,'source_sha256':{str(p.relative_to(repo)):hashlib.sha256(p.read_bytes()).hexdigest()for p in files},'oracle_sha256':hashlib.sha256(go.read_bytes()).hexdigest(),'records':records,'cli':cli,'negative_controls':controls,'native_macos_windows':'native exact-head CI required; Linux alone does not establish these targets','remaining_scope':'distinct Copilot tokens, unsafe/unreadable sources, unproven device headers, home mismatch on Windows, base/workspace/numeric expiry and automatic host Keychain remain gated; full #768 remains open'}
output.write_text(json.dumps(receipt,indent=2)+'\n')
