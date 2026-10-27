"""Compare current Go/Rust CLI diagnostics with synthetic, unusable stores only.

No case contains a usable credential or selected Copilot account, so this
runner cannot read Keychain values or issue authenticated provider requests.
Positive usage and injected-Keychain paths live in the Go/Rust fixture tests.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("go_binary", type=Path)
parser.add_argument("rust_binary", type=Path)
parser.add_argument("output", type=Path)
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
corpus = json.loads((repo / "rust/symbrain-usage/tests/fixtures/current_copilot_cli.json").read_text())
cases = [("copilot-" + c["id"], ".copilot/config.json", c["data"])
         for c in corpus if c["account"] == ""]
cases += [("nous-" + name, ".hermes/auth.json", data) for name, data in [
    ("signed-out", '{"providers":{"nous":{"last_auth_error":{"message":"private fixture diagnostic"}}}}'),
    ("expired", '{"providers":{"nous":{"access_token":"fixture-expired","expires_at":"2000-01-01T00:00:00Z"}}}'),
    ("empty-pool", '{"providers":{},"credential_pool":{"nous":[]}}'),
    ("invalid-jwt", '{"providers":{"nous":{"access_token":"invalid.jwt.fixture"}}}'),
]]
assert len(cases) == 11
args.output.parent.mkdir(parents=True, exist_ok=True)
records = []
with tempfile.TemporaryDirectory(prefix="current-cli-homes-", dir=args.output.parent) as scratch:
    for name, relative, contents in cases:
        home = Path(scratch) / name
        path = home / relative
        path.parent.mkdir(parents=True)
        path.write_text(contents)
        env = {k: v for k, v in os.environ.items() if k in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP", "TMPDIR", "PATHEXT"]}
        env.update(HOME=str(home), USERPROFILE=str(home), HERMES_HOME=str(home / ".hermes"),
                   PATH="", ANTHROPIC_OAUTH_TOKEN="env://CURRENT_CLI_ABSENT",
                   SYMBRAIN_GO_BINARY=str(home / "absent-go"))
        for mode, flags in [("table", []), ("json", ["--json"])]:
            observed = []
            for binary in [args.go_binary, args.rust_binary]:
                result = subprocess.run([str(binary.resolve()), "usage", *flags], env=env,
                                        capture_output=True, check=False, timeout=15)
                observed.append({"exit": result.returncode,
                                 "stdout_b64": base64.b64encode(result.stdout).decode(),
                                 "stderr_b64": base64.b64encode(result.stderr).decode()})
            records.append({"id": name + "-" + mode, "matches": observed[0] == observed[1],
                            "go": observed[0], "rust": observed[1]})
        assert path.read_text() == contents, "credential store mutated"
report = {"cases": len(records), "passed": sum(r["matches"] for r in records),
          "failed": sum(not r["matches"] for r in records), "records": records}
args.output.write_text(json.dumps(report, indent=2) + "\n")
assert len(records) == 22 and report["failed"] == 0, "current Go/Rust CLI mismatch"
print(f"current CLI parity: {report['passed']}/{report['cases']} byte/exit matches")
