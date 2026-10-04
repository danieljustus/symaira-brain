"""Bind fresh constructor/request replay to immutable source and candidate."""
import hashlib
import json
import pathlib
import platform
import subprocess
import sys

oracle, native, output = map(pathlib.Path, sys.argv[1:4])
cases = json.loads(oracle.read_text())
result = json.loads(native.read_text())
controls = json.loads(oracle.with_name("controls.json").read_text())
assert len(controls) == 5 and all(control["exit"] != 0 for control in controls)
assert len(cases) == 44 and result == {"cases": 44, "passed": 44, "failed": 0}
repo = pathlib.Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
files = sorted((repo / "rust/symbrain-usage/src").rglob("*.rs")) + [repo / path for path in ["rust/symbrain-cli/src/usage_cli.rs", "rust/symbrain-cli/src/lib.rs","rust/symbrain-cli/src/flag_normalization.rs", "Cargo.lock", "rust/symbrain-usage/Cargo.toml", "scripts/usage-credential-oracle/provider_test.go.txt", "scripts/usage-credential-oracle/run.sh", "rust/symbrain-usage/tests/credential_reference_tests.rs"]]
files += [repo/path for path in ['rust/symbrain-core/src/config/format.rs', 'rust/symbrain-core/src/config/set.rs', 'rust/symbrain-core/src/config/mod.rs', 'rust/symbrain-core/src/go_printable.rs']]
receipt = {"schema_version": 1, "oracle_commit": sys.argv[4], "candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(), "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()), "native_platform": sys.platform, "native_arch": platform.machine(), "go_sdk": subprocess.check_output(["go", "version"], text=True).strip(), "rust_sdk": subprocess.check_output(["rustc", "--version"], text=True).strip(), "constructor_request_cases": 44, "passed": 44, "failed": 0, "source_sha256": {str(path.relative_to(repo)): hashlib.sha256(path.read_bytes()).hexdigest() for path in files}, "oracle_sha256": hashlib.sha256(oracle.read_bytes()).hexdigest(), "oracle_cases": cases, "cli": json.loads(oracle.with_name("cli.json").read_text()), "negative_controls": controls}
output.write_text(json.dumps(receipt, indent=2) + "\n")
