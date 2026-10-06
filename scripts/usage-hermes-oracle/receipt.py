"""Retain full immutable-Go input, report, request and read-only evidence."""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys

go,native,output=map(pathlib.Path,sys.argv[1:4])
records=json.loads(go.read_text(encoding='utf-8'))
assert len(records)==96 and json.loads(native.read_text(encoding='utf-8'))=={"cases":96,"passed":96,"failed":0}
cli=json.loads(go.with_name('cli.json').read_text(encoding='utf-8'))
controls=json.loads(go.with_name('controls.json').read_text(encoding='utf-8'))
assert cli['cases']==104 and cli['passed']==104 and cli['failed']==0 and len(controls)==5 and all(item['exit']!=0 for item in controls)
repo=pathlib.Path(subprocess.check_output(["git","rev-parse","--show-toplevel"],text=True, encoding='utf-8').strip())
files=sorted((repo/"rust/symbrain-usage/src").rglob("*.rs"))+[repo/path for path in ["rust/symbrain-usage/Cargo.toml","Cargo.lock","rust/symbrain-usage/tests/hermes_credential_tests.rs","rust/symbrain-cli/tests/mcp_cli_tests.rs","rust/symbrain-cli/src/usage_cli.rs","rust/symbrain-cli/src/lib.rs","scripts/usage-hermes-oracle/cases.py","scripts/usage-hermes-oracle/provider_test.go.txt","scripts/usage-hermes-oracle/run.sh","scripts/usage-hermes-oracle/cli.py","scripts/usage-hermes-oracle/receipt.py"]]
files += [repo/path for path in ['rust/symbrain-core/src/config/format.rs', 'rust/symbrain-core/src/config/set.rs', 'rust/symbrain-core/src/config/mod.rs', 'rust/symbrain-core/src/go_printable.rs']]
receipt={"schema_version":1,"oracle_commit":sys.argv[4],"candidate_head":subprocess.check_output(["git","rev-parse","HEAD"],text=True, encoding='utf-8').strip(),"candidate_dirty":bool(subprocess.check_output(["git","status","--porcelain"],text=True, encoding='utf-8').strip()),"platform":sys.platform,"arch":platform.machine(),"go_sdk":subprocess.check_output(["go","version"],text=True, encoding='utf-8').strip(),"rust_sdk":subprocess.check_output(["rustc","--version"],text=True, encoding='utf-8').strip(),"cases":96,"passed":96,"failed":0,"source_sha256":{str(path.relative_to(repo)):hashlib.sha256(path.read_bytes()).hexdigest()for path in files},"oracle_sha256":hashlib.sha256(go.read_bytes()).hexdigest(),"records":records,"cli":cli,"negative_controls":controls,"filesystem":json.loads(go.with_name("filesystem.json").read_text(encoding='utf-8'))}
output.write_text(json.dumps(receipt,indent=2)+"\n", encoding='utf-8')
