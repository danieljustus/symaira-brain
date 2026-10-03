#!/usr/bin/env python3
"""Execute immutable Go evidence contracts and verify the additive fixture."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

ORACLE = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
COREKIT_SHA = "8316336a261e6ab9ff8c525bdf8cae09c052a71c938d93204870b1b4d1808e54"


def run(args, cwd, env=None):
    return subprocess.run(args, cwd=cwd, env=env, check=True, capture_output=True)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", type=Path)
    parser.add_argument("--fixture", type=Path, help="explicit fixture for fail-closed replay controls")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    go_cache = run(["go", "env", "GOMODCACHE", "GOCACHE"], repo).stdout.decode().splitlines()
    corekit = Path(go_cache[0]) / "github.com/danieljustus/symaira-corekit@v0.17.0/evidencekit/evidencekit.go"
    archive = run(["git", "archive", ORACLE], repo).stdout
    with tempfile.TemporaryDirectory(prefix="memory-evidence-oracle-") as temporary:
        root = Path(temporary)
        source = root / "source"
        source.mkdir()
        with tarfile.open(fileobj=io.BytesIO(archive)) as files:
            files.extractall(source, filter="data")
        generator = source / "scripts/memory-evidence-oracle/main.go"
        generator.parent.mkdir(parents=True, exist_ok=True)
        generator.write_bytes((repo / "scripts/memory-evidence-oracle/main.go.txt").read_bytes())
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("SYMMEMORY_", "SYMBRAIN_MEMORY_"))}
        home = root / "home"
        home.mkdir()
        env.update(HOME=str(home), USERPROFILE=str(home), GOWORK="off", GOENV="off",
                   GOMODCACHE=go_cache[0], GOCACHE=go_cache[1], CGO_ENABLED="0")
        for key, folder in (("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                            ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state")):
            env[key] = str(home / folder)
        run(["go", "mod", "download", "github.com/danieljustus/symaira-corekit@v0.17.0"], source, env)
        assert digest(corekit.read_bytes()) == COREKIT_SHA, "pinned evidencekit source changed"
        generated = run(["go", "run", "./scripts/memory-evidence-oracle"], source, env).stdout
        fixture = args.fixture or repo / "rust/symbrain-memory/tests/fixtures/memory_evidence_go_v017.json"
        expected = fixture.read_bytes()
        assert generated == expected, "actual Go output differs from immutable additive fixture"
        value = json.loads(generated)
        assert len(value["alignments"]) == 32 and len(value["validations"]) == 48
        assert len(value["extractions"]) == 3, "oracle must execute every JSONL record"
        # These existing Go tests exercise the production DB, strict ingestion,
        # cascading delete and reparenting. No Go test source is altered.
        tests = run(["go", "test", "-json", "./internal/memory/db", "-run",
                     "^(TestSaveMemoryEvidence_|TestMemoryEvidence_|TestReparentMemoryEvidenceTx_)",
                     "-count=1"], source, env).stdout
        passed = [json.loads(line)["Test"] for line in tests.splitlines()
                  if json.loads(line).get("Action") == "pass" and "Test" in json.loads(line)]
        assert len(passed) == 4, f"expected four production Go tests, got {passed}"
        source_hashes = {str(path.relative_to(source)): digest(path.read_bytes())
                         for path in (source / "internal/memory/db").glob("*.go")}
        candidate_files = run(["git", "ls-files", "rust/symbrain-memory", "Cargo.toml", "Cargo.lock",
                               "scripts/memory-evidence-oracle", ".github/workflows/memory-evidence-native.yml"], repo).stdout.decode().splitlines()
        report = {
            "candidate_source_sha256": {name: digest((repo / name).read_bytes()) for name in candidate_files},
            "immutable_go_db_source_sha256": source_hashes,
            "go_oracle_revision": ORACLE,
            "corekit_version": "v0.17.0",
            "corekit_evidence_source_sha256": COREKIT_SHA,
            "generator_sha256": digest(generator.read_bytes()),
            "fixture_sha256": digest(generated),
            "candidate_revision": run(["git", "rev-parse", "HEAD"], repo).stdout.decode().strip(),
            "candidate_dirty": bool(run(["git", "status", "--porcelain"], repo).stdout),
            "go_version": run(["go", "version"], repo).stdout.decode().strip(),
            "alignment_cases": len(value["alignments"]),
            "validation_cases": len(value["validations"]),
            "jsonl_records": len(value["extractions"]),
            "jsonl_sha256": value["jsonl_sha256"],
            "production_go_db_tests_passed": passed,
            "operator_home_used": False,
        }
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
