#!/usr/bin/env python3
"""Allocated full local runtime driver; never treats static lineage as execution."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

FROZEN = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
PACKAGES = ("symbrain-memory", "symbrain-cli", "symbrain-gateway", "symbrain-mcp")
SDK_ENV = ("PATH", "SystemRoot", "WINDIR", "CARGO_HOME", "RUSTUP_HOME",
           "GOMODCACHE", "GOCACHE", "RUSTUP_TOOLCHAIN", "RUSTC", "RUSTDOC",
           "LD_LIBRARY_PATH", "LIBRARY_PATH", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET",
           "SSL_CERT_FILE", "SSL_CERT_DIR")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.write_bytes((json.dumps(value, indent=2, sort_keys=True) + "\n").encode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("go", "go-source", "target", "output", "actionlint"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--go-sha256", required=True,
                        help="actual oracle hash from its preserved source/build receipt")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    for name in ("go", "go_source", "target", "output", "actionlint"):
        setattr(args, name, getattr(args, name).resolve())
    # Resource ownership (including port 11434) is allocated by the coordinator
    # before invocation, not inferred by this driver or acquired by probing it.
    assert args.target.is_dir(), "only an explicitly released existing target is allowed"
    assert not args.output.is_relative_to(repo), "keep runtime reports outside the source checkout"
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=repo), "freeze clean source first"
    assert digest(args.go) == args.go_sha256, "supplied oracle differs from its preserved binding"
    args.output.mkdir()  # Retain a failed earlier run; never silently reuse it.
    owned = args.output / "owned-environment"
    owned.mkdir()
    environment = {key: os.environ[key] for key in SDK_ENV if key in os.environ}
    for key in ("CARGO_HOME", "RUSTUP_HOME", "GOMODCACHE", "GOCACHE"):
        assert environment.get(key), "provide allocated SDK cache explicitly: " + key
    environment.update(HOME=str(owned), USERPROFILE=str(owned), TZ="UTC", LANG="C.UTF-8",
                       CARGO_TARGET_DIR=str(args.target), CARGO_BUILD_JOBS="2",
                       CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0",
                       CARGO_PROFILE_TEST_DEBUG="0", GOWORK="off", GOENV="off",
                       CGO_ENABLED="0", SYMBRAIN_GO_BINARY=str(owned / "absent-fallback"))
    for suffix in ("CONFIG", "DATA", "CACHE", "STATE"):
        path = owned / suffix.lower()
        path.mkdir()
        environment["XDG_" + suffix + "_HOME"] = str(path)
    source_paths = subprocess.check_output([
        "git", "ls-files", "rust", "scripts/memory-historical-oracle",
        "scripts/memory-cli-oracle", "scripts/memory-evidence-oracle",
        ".github/workflows", "Cargo.toml", "Cargo.lock", ".cargo"], cwd=repo,
        text=True).splitlines()
    source_hashes = {name: digest(repo / name) for name in source_paths}
    migration_tests = []
    for path in sorted((repo / "rust/symbrain-memory/src/migration").glob("*.rs")):
        for name in re.findall(r"#\[test\]\s*fn\s+(\w+)\s*\(", path.read_text()):
            migration_tests.append("migration::" + path.stem + "::" + name)
    assert len(migration_tests) == 28, "reconcile complete historical test inventory"
    expected_tests = list(migration_tests)
    if os.name == "nt":
        # Source retains this Unix-only permission test; Windows must not be
        # reported as executing a test excluded by its real Rust cfg.
        expected_tests.remove("migration::rollback_tests::successful_public_open_secures_existing_database_without_rewriting_its_rows")
    resources = sorted((repo / "rust/symbrain-memory/src/migration/sql").glob("*.sql"))
    assert len(resources) == 37
    for path in resources:
        frozen = subprocess.check_output(["git", "show", FROZEN + ":internal/memory/db/migrations/" + path.name], cwd=repo)
        assert path.read_bytes() == (args.go_source / "internal/memory/db/migrations" / path.name).read_bytes() == frozen
    receipt = dict(source=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
                   source_hashes=source_hashes, frozen_go=FROZEN,
                   oracle=dict(path=str(args.go), sha256=digest(args.go), binding=args.go_sha256),
                   owned_resources={path.name: digest(path) for path in resources},
                   prepared_historical_test_names=migration_tests,
                   expected_native_historical_test_names=expected_tests, environment=environment,
                   stages=[], started=datetime.now(timezone.utc).isoformat(),
                   native_other_OS_runtime_claim=False, full649_or758_claim=False)
    receipt_path = args.output / "validation.json"
    save(receipt_path, receipt)

    def run(name, command):
        free = shutil.disk_usage(args.target).free
        assert free >= 700 * 1024 * 1024, "stage stopped below 700 MiB: " + name
        log = args.output / (name + ".log")
        stage = dict(name=name, argv=list(map(str, command)), free_before=free,
                     started=datetime.now(timezone.utc).isoformat(), log=str(log))
        receipt["stages"].append(stage)
        save(receipt_path, receipt)
        with log.open("wb") as stream:
            process = subprocess.run(stage["argv"], cwd=repo, env=environment,
                                     stdout=stream, stderr=subprocess.STDOUT, check=False)
        stage.update(exit=process.returncode, log_sha256=digest(log),
                     ended=datetime.now(timezone.utc).isoformat(),
                     free_after=shutil.disk_usage(args.target).free)
        save(receipt_path, receipt)
        assert process.returncode == 0, "failed stage retained: " + name
        assert stage["free_after"] >= 700 * 1024 * 1024, "stage ended below 700 MiB"
        return log

    for tool, command in (("rust-sdk", ["rustc", "-Vv"]), ("cargo-sdk", ["cargo", "-V"]),
                          ("go-sdk", ["go", "version"])):
        log = run(tool, command)
        if tool == "go-sdk":
            assert " go1.26.7 " in log.read_text(), "use the pinned actual Go SDK"
    package_flags = [value for package in PACKAGES for value in ("-p", package)]
    tests = run("all-target-tests", ["cargo", "test", *package_flags,
                                     "--all-targets", "--all-features", "--locked", "--", "--nocapture"])
    raw_tests = tests.read_text(errors="strict")
    summaries = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", raw_tests)
    assert summaries and all(failed == ignored == "0" for _, failed, ignored in summaries)
    for name in expected_tests:
        assert "test " + name + " ... ok" in raw_tests, "historical test not executed: " + name
    executed = []
    for match in re.finditer(r"Running [^\n]* \(([^\n]+)\)", raw_tests):
        path = Path(match.group(1))
        if not path.is_absolute():
            path = repo / path
        assert path.is_file(), "actual Cargo executable path missing: " + str(path)
        executed.append(dict(path=str(path.resolve()), sha256=digest(path)))
    assert executed, "bind actual executed Cargo files, not just compiler artifacts"
    receipt["tests"] = dict(passed=sum(int(passed) for passed, _, _ in summaries),
                            failed=0, ignored=0, summaries=len(summaries),
                            actual_executed_binaries=executed,
                            historical_tests_executed=len(expected_tests))
    save(receipt_path, receipt)
    run("strict-clippy", ["cargo", "clippy", *package_flags,
                          "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"])
    run("fmt", ["cargo", "fmt", "--all", "--", "--check"])
    run("actionlint", [args.actionlint])
    run("diffcheck", ["git", "diff", "--check"])
    # Test harnesses alone must not bind a stale production CLI from the cache.
    run("build-actual-cli", ["cargo", "build", "-p", "symbrain-cli", "--locked"])
    native = args.target / "debug" / ("symbrain.exe" if os.name == "nt" else "symbrain")
    receipt["actual_cli"] = dict(path=str(native), sha256=digest(native))
    save(receipt_path, receipt)
    oracle = repo / "scripts/memory-historical-oracle"
    common = ["--go", args.go, "--go-source", args.go_source, "--native", native]
    run("historical-37-pairs-reopens", [sys.executable, oracle / "replay.py", *common,
                                       "--output", args.output / "historical"])
    run("historical-nine-controls", [sys.executable, oracle / "controls.py", *common,
                                      "--output", args.output / "historical-controls"])
    run("owned-defaults-sql-inventory", [sys.executable, oracle / "defaults_inventory.py",
                                         "--output", args.output / "defaults-sql"])
    cli = repo / "scripts/memory-cli-oracle"
    run("memory-cli-reads", [sys.executable, cli / "replay.py", "--go", args.go,
                              "--rust", native, "--report", args.output / "reads.json"])
    run("memory-cli-two-controls", [sys.executable, cli / "controls.py", "--go", args.go,
                                     "--rust", native, "--report", args.output / "read-controls.json"])
    run("memory-cli-writes-controls", [sys.executable, cli / "write_gate.py", "--go", args.go,
                                        "--rust", native, "--report-dir", args.output / "writes"])
    run("evidence-actual-go", [sys.executable, repo / "scripts/memory-evidence-oracle/replay.py",
                               "--report", args.output / "evidence.json"])
    run("evidence-three-controls", [sys.executable, repo / "scripts/memory-evidence-oracle/controls.py",
                                     "--report", args.output / "evidence-controls.json"])
    assert {name: digest(repo / name) for name in source_paths} == source_hashes
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)
    assert digest(native) == receipt["actual_cli"]["sha256"]
    receipt["finished"] = datetime.now(timezone.utc).isoformat()
    receipt["report_files"] = {str(path.relative_to(args.output)): digest(path)
                               for path in sorted(args.output.rglob("*"))
                               if path.is_file() and path != receipt_path}
    save(receipt_path, receipt)
    print(json.dumps(dict(tests=receipt["tests"]["passed"], historical_tests=len(expected_tests),
                          historical_pairs=37, native_reopens=37, historical_controls=9,
                          native_OS=os.name, other_OS_runtime_claim=False)))


if __name__ == "__main__":
    main()
