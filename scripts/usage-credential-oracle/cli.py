"""Real Go/native CLI failure bytes in disposable roots; no HTTP credentials."""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("go_binary", type=pathlib.Path)
parser.add_argument("rust_binary", type=pathlib.Path)
parser.add_argument("fake_binary", type=pathlib.Path)
parser.add_argument("output", type=pathlib.Path)
parser.add_argument("--control", choices=["exit", "missing-case"])
args = parser.parse_args()
names = ["ANTHROPIC_ADMIN_KEY", "ANTHROPIC_OAUTH_TOKEN", "CODEX_ACCESS_TOKEN", "COPILOT_ACCESS_TOKEN", "CURSOR_COOKIE", "KIMI_CODE_API_KEY", "KIMI_AUTH_TOKEN", "MOONSHOT_API_KEY", "NOUS_PORTAL_ACCESS_TOKEN", "OPENCODE_COOKIE", "OPENROUTER_API_KEY"]
variants = {"env-missing": "env://USAGE_768_ABSENT", "env-empty": "env://", "vault-empty-path": "symvault://", "vault-leading-dash": "vault://-bad", "vault-control": "symvault://bad\npath", "keychain-malformed": "keychain://missing-account", "keychain-failed": "keychain://test/account", "vault-failed": "symvault://test/token", "vault-empty-output": "symvault://test/token", "vault-absent-binary": "symvault://test/token"}
records = []


def reference_resolution_divergence(go, native):
    """#930 deliberately fixes the frozen oracle's missing-credential report.

    Keep the pinned source unchanged. Accept only configured/error promotions
    whose diagnostic is already present verbatim in the oracle's auth status;
    every other report field, exit status and stderr must still agree.
    """
    if go["exit"] != native["exit"] or go["stderr_b64"] != native["stderr_b64"]:
        return False
    try:
        expected = json.loads(base64.b64decode(go["stdout_b64"]))
        actual = json.loads(base64.b64decode(native["stdout_b64"]))
    except (ValueError, UnicodeError):
        return False
    promoted = 0
    for provider in expected.get("providers", []):
        detail = provider.get("auth_status", {}).get("detail", "")
        if detail.startswith("credential resolution failed: ") and "error" not in provider:
            provider["configured"] = True
            provider["error"] = detail
            promoted += 1
    return promoted > 0 and expected == actual


with tempfile.TemporaryDirectory(prefix="usage-credential-cli-768-") as scratch:
    root = pathlib.Path(scratch)
    binary_dir = root / "bin"
    binary_dir.mkdir()
    extension = ".exe" if os.name == "nt" else ""
    for name in ["symvault", "security"]:
        image = binary_dir / (name + extension)
        image.write_bytes(args.fake_binary.read_bytes())
        image.chmod(0o755)
    empty_bin = root / "empty-bin"
    empty_bin.mkdir()
    for name in names:
        for variant, reference in variants.items():
            home = root / (name + "-" + variant)
            home.mkdir()
            env = {key: value for key, value in os.environ.items() if key in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP", "TMPDIR", "PATHEXT"]}
            env.update({"HOME": str(home), "USERPROFILE": str(home), "XDG_CONFIG_HOME": str(home / "config"), "XDG_DATA_HOME": str(home / "data"), "XDG_CACHE_HOME": str(home / "cache"), "PATH": str(empty_bin if variant == "vault-absent-binary" else binary_dir), "ANTHROPIC_OAUTH_TOKEN": "env://USAGE_768_ABSENT", "SYMBRAIN_GO_BINARY": str(empty_bin / "absent-go"), "SECRET_ORACLE_FAKE_ARGS_PATH": str(home / "args.log"), "SECRET_ORACLE_FAKE_STDOUT": " \t\n", "SECRET_ORACLE_FAKE_STDERR": "synthetic-768-rejected" if variant in ["vault-failed", "keychain-failed"] else "", "SECRET_ORACLE_FAKE_EXIT": "7" if variant in ["vault-failed", "keychain-failed"] else "0"})
            env[name] = reference
            observed = []
            for binary in [args.go_binary, args.rust_binary]:
                result = subprocess.run([str(binary.resolve()), "usage", "--json"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, check=False)
                observed.append({"exit": result.returncode, "stdout_b64": base64.b64encode(result.stdout).decode(), "stderr_b64": base64.b64encode(result.stderr).decode()})
            if args.control == "exit" and not records:
                observed[0]["exit"] = 42
            byte_matches = observed[0] == observed[1]
            known_divergence = not byte_matches and reference_resolution_divergence(*observed)
            records.append({"id": name + "-" + variant, "go": observed[0], "rust": observed[1], "matches": byte_matches or known_divergence, "byte_matches": byte_matches, **({"known_divergence": "#930: explicit credential reference failure is configured with error"} if known_divergence else {})})
if args.control == "missing-case":
    records.pop()
expected = len(names) * len(variants)
report = {"schema_version": 1, "cases": len(records), "expected_cases": expected, "passed": sum(record["matches"] for record in records), "failed": sum(not record["matches"] for record in records), "platform": sys.platform, "binary_sha256": {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in [("go", args.go_binary), ("rust", args.rust_binary)]}, "cases_detail": records}
report["byte_matches"] = sum(record["byte_matches"] for record in records)
report["known_divergences"] = sum("known_divergence" in record for record in records)
args.output.write_text(json.dumps(report, indent=2) + "\n", encoding='utf-8')
if len(records) != expected:
    sys.exit("usage credential oracle: missing CLI case")
if report["failed"]:
    sys.exit("usage credential oracle: CLI byte/exit mismatch: " + ", ".join(record["id"] for record in records if not record["matches"]))
print(f"usage credential CLI oracle: {report['byte_matches']}/{expected} byte/exit matches; {report['known_divergences']} known #930 divergences")
