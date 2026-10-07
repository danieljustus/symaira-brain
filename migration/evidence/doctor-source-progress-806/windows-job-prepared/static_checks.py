"""Source-only preparation; never builds or invokes a provider/product binary."""
import ast
import contextlib
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[4]
base = "8359c0bda1b7a0bfc14d2b8e69e1479fec9d182f"
sha = lambda raw: hashlib.sha256(raw).hexdigest()
folder = root / "migration/evidence/doctor-source-progress-806"
preserved = 0
original = json.loads((folder / "windows-job111372954990-original/receipt.json").read_bytes())
for row in original["original_files"]:
    raw_gzip = (folder / "windows-job111372954990-original" / row["retained"]).read_bytes()
    raw = gzip.decompress(raw_gzip)
    assert sha(raw_gzip) == row["gzip_sha256"]
    assert sha(raw) == row["sha256"] and len(raw) == row["bytes"]
    preserved += 1
for path, expected in original["actual_source_maps"].items():
    assert sha(subprocess.check_output(["git", "show", base + ":" + path], cwd=root)) == expected
import_retention = json.loads((folder / "import-boundary-8359/retention.json").read_bytes())
for row in import_retention:
    compressed = (root / row["archive"]).read_bytes()
    raw = gzip.decompress(compressed)
    assert sha(compressed) == row["gzip_sha256"]
    assert sha(raw) == row["sha256"] and len(raw) == row["bytes"]
changes = subprocess.check_output(["git", "diff", base, "--name-only"], cwd=root, text=True).splitlines()
assert not any(path.startswith(("rust/symbrain-cli/src/", "cmd/", "internal/")) or path.endswith((".go", "Cargo.lock", "Cargo.toml")) for path in changes)
replay = root / "scripts/setup-source-oracle/replay.py"
old = subprocess.check_output(["git", "show", base + ":scripts/setup-source-oracle/replay.py"], cwd=root)
def functions(raw):
    return {n.name:ast.dump(n, include_attributes=False) for n in ast.parse(raw).body
            if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef))}
assert functions(old) == functions(replay.read_bytes())
assert "timeout=25" in replay.read_text()
probe = (root / "scripts/setup-source-oracle/windows_job_probe.py").read_text()
assert "child.wait(timeout=12)" in probe and "child.wait(timeout=5)" in probe
assert 'production_acceptance=False' in probe and 'original_cli_timeout_cause_proved=False' in probe
assert 'owned_image_name_matches' in probe
test = (root / "rust/symbrain-cli/tests/source_job_notifications.rs").read_text()
assert "for _ in 0..16" in test and "let killed = self.0.kill();" in test and "let waited = self.0.wait();" in test
for phase in ["historical-kill-enter", "historical-kill-returned", "historical-wait-enter", "historical-wait-returned", "job-termination-enter", "job-termination-returned", "inner-wait-enter", "inner-wait-result"]:
    assert phase in test
driver = (root / "scripts/setup-source-oracle/windows_job_run.sh").read_text()
selector = driver.split("<<'PY'\n", 1)[1].split("\nPY\n", 1)[0]
selector_results = []
with tempfile.TemporaryDirectory(prefix="doctor806-artifact-selection-") as directory:
    directory = Path(directory)
    binary = directory / "owned-synthetic-artifact-not-executed.exe"
    binary.write_bytes(b"synthetic artifact path selection only\n")
    artifact = dict(reason="compiler-artifact", target=dict(name="source_job_notifications", kind=["test"], src_path=str(root / "rust/symbrain-cli/tests/source_job_notifications.rs")), executable=str(binary))
    for label, rows, valid in [("exact", [artifact], True), ("duplicate", [artifact, artifact], False),
                              ("wrong-source", [{**artifact, "target": {**artifact["target"], "src_path": str(directory / "unrelated.rs")}}], False),
                              ("no-executable", [{**artifact, "executable": None}], False)]:
        report = directory / "cargo.jsonl"
        report.write_text("cargo diagnostic non-JSON line\n" + "\n".join(map(json.dumps, rows)))
        argv = sys.argv
        try:
            sys.argv = ["owned-selector", str(report)]
            with contextlib.redirect_stdout(io.StringIO()) as output:
                try:
                    exec(compile(selector, "windows_job_run.sh owned selector", "exec"), {})
                    passed = True
                except AssertionError:
                    passed = False
            assert passed == valid
            if valid:
                assert output.getvalue().strip() == str(binary.resolve())
            selector_results.append(dict(case=label, accepted=passed))
        finally:
            sys.argv = argv
for path in (root / "scripts/setup-source-oracle").glob("*.py"):
    ast.parse(path.read_text(), str(path))
checks = [(["/home/agent/.cargo/bin/rustfmt", "--edition", "2024", "--check", "rust/symbrain-cli/tests/source_job_notifications.rs"], "rustfmt"),
          (["bash", "-n", "scripts/setup-source-oracle/windows_job_run.sh"], "bash"),
          (["/workspace/toolchains/bin/actionlint", ".github/workflows/ci.yml"], "actionlint"),
          (["git", "diff", "--check"], "diff-check")]
results = []
for command, label in checks:
    result = subprocess.run(command, cwd=root, capture_output=True)
    assert result.returncode == 0, (label, result.stdout, result.stderr)
    results.append(dict(check=label, exit=result.returncode, stdout_hex=result.stdout.hex(), stderr_hex=result.stderr.hex()))
sources = [".github/workflows/ci.yml", "scripts/setup-source-oracle/windows_job_run.sh", "scripts/setup-source-oracle/windows_job_probe.py", "rust/symbrain-cli/tests/source_job_notifications.rs", "scripts/setup-source-oracle/replay.py", "rust/symbrain-cli/src/setup_source_process.rs", "Cargo.lock", "docs/adr/doctor-source-timeout-observability-806.md"]
receipt = dict(scope="Source-only Windows diagnostic preparation; no production acceptance", base=base,
               actual_original_source_maps_verified=10, original_windows_full_maps=preserved,
               root_import_full_maps=len(import_retention), replay_unchanged_ASTs=len(functions(old)),
               compiler_artifact_selector_only=selector_results, checks=results,
               source_sha256={p:sha((root/p).read_bytes()) for p in sources},
               compiler_runs=0, product_provider_runs=0, ports=0, native_windows_runs=0,
               original_source_timeout_seconds=25, diagnostic_watchdog_seconds=12,
               limitations="Actual Windows job diagnostics pending. Original polling-versus-cleanup cause remains unproved; no PE payloads in downloaded original artifact.")
(Path(__file__).parent / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps(receipt, indent=2))
