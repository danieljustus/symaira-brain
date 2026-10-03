"""Source-bound Go/native file reports, literal requests, CLI and rejection proof."""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys

go,native,output=map(pathlib.Path,sys.argv[1:4])
records=json.loads(go.read_text())
assert len(records)==86 and json.loads(native.read_text())==dict(cases=86,passed=86,failed=0,full_reports=85,gated=1)
cli=json.loads(go.with_name('cli.json').read_text())
controls=json.loads(go.with_name('controls.json').read_text())
assert cli['cases']==cli['expected_cases']==cli['passed']==151 and cli['failed']==0 and cli['route_cases']==85
assert len(controls)==5 and [item['exit']for item in controls]==[1,1,101,101,101]
repo=pathlib.Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())
files=sorted((repo/'rust/symbrain-usage/src').rglob('*.rs'))+[repo/path for path in ['rust/symbrain-usage/Cargo.toml','Cargo.lock','rust/symbrain-cli/tests/mcp_cli_tests.rs','rust/symbrain-cli/src/usage_cli.rs','rust/symbrain-cli/src/lib.rs']]+sorted((repo/'scripts/usage-provider-files-oracle').glob('*'))
files=[path for path in files if path.is_file()]
receipt={'schema_version':1,'oracle_commit':sys.argv[4],'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'candidate_dirty':bool(subprocess.check_output(['git','status','--porcelain'],text=True).strip()),'platform':sys.platform,'arch':platform.machine(),'go_sdk':subprocess.check_output(['go','version'],text=True).strip(),'rust_sdk':subprocess.check_output(['rustc','--version'],text=True).strip(),'cases':86,'passed':86,'failed':0,'full_report_cases':85,'retained_ambiguity_gates':1,'source_sha256':{str(path.relative_to(repo)):hashlib.sha256(path.read_bytes()).hexdigest()for path in files},'oracle_sha256':hashlib.sha256(go.read_bytes()).hexdigest(),'records':records,'cli':cli,'negative_controls':controls,'filesystem':json.loads(go.with_name('filesystem.json').read_text()),'keychain_scope':'private unchanged Go constructor injection and equivalent native private adapter; host discovery/ACL behavior remains gated'}
output.write_text(json.dumps(receipt,indent=2)+'\n')
