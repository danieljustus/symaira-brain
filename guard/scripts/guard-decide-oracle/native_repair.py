#!/usr/bin/env python3
"""Native CI: build immutable Go oracle and candidate; execute F01-F05 gates."""
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

import oracle
from trust_anchor import TrustAnchor, file_sha256, load_anchor


ROOT = Path(__file__).resolve().parents[3]
ANCHOR_PATH = (
    ROOT / "rust/symbrain-guard-core/tests/fixtures/external_repair_trust_anchor.rs"
)
ANCHOR: TrustAnchor = load_anchor(ANCHOR_PATH)
PIN = ANCHOR.oracle_commit
GO_TOOLCHAIN = ANCHOR.go_toolchain
FIXTURE = ROOT / "rust/symbrain-guard-core/tests/fixtures/external_repair_oracle.json"
GENERATOR = ROOT / "guard/scripts/guard-decide-oracle/repair_parity.py"
VALIDATOR = Path(__file__).resolve()
OUT = Path(
    os.environ.get(
        "GUARD_REPAIR_OUTPUT", ROOT / "target/migration-run/guard-decide-raw-byte"
    )
).resolve()
OUT.mkdir(parents=True, exist_ok=True)


def verify_trust_anchor() -> None:
    """Check candidate evidence against literals compiled by the Rust tests."""

    mismatches = []
    if not FIXTURE.is_file():
        mismatches.append("fixture missing")
    elif file_sha256(FIXTURE) != ANCHOR.fixture_sha256:
        mismatches.append("fixture digest mismatch")
    if not GENERATOR.is_file() or file_sha256(GENERATOR, normalize_crlf=True) != ANCHOR.generator_sha256:
        mismatches.append("generator digest mismatch")
    if not VALIDATOR.is_file() or file_sha256(VALIDATOR, normalize_crlf=True) != ANCHOR.validator_sha256:
        mismatches.append("validator digest mismatch")
    if mismatches:
        raise AssertionError("trusted Rust anchor mismatch: " + ", ".join(mismatches))


def verify_source_files(source: Path) -> dict[str, str]:
    actual = {}
    for relative, expected in ANCHOR.source_files:
        path = source / relative
        if not path.is_file():
            raise AssertionError(f"trusted source file is missing: {relative}")
        actual[relative] = file_sha256(path)
        if actual[relative] != expected:
            raise AssertionError(f"trusted source file digest mismatch: {relative}")
    return actual


def main():
    verify_trust_anchor()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    env = dict(os.environ)
    env["RUSTUP_HOME"] = os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup"))
    # Build tools retain installation/toolchain roots; all writable application
    # and Go build caches are isolated. Runtime probes use a narrower allowlist.
    for key, name in [
        ("HOME", "home"),
        ("USERPROFILE", "profile"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("GOCACHE", "gocache"),
        ("TMP", "tmp"),
        ("TEMP", "tmp"),
        ("TMPDIR", "tmp"),
        ("CARGO_TARGET_DIR", "target"),
    ]:
        path = OUT / name
        path.mkdir(exist_ok=True)
        env[key] = str(path)
    env.update(
        GOTOOLCHAIN=GO_TOOLCHAIN,
        GOENV="off",
        GOFLAGS="",
        GOWORK="off",
        GOMODCACHE=str(OUT / "gomodcache"),
        GOPATH=str(OUT / "gopath"),
        CARGO_BUILD_JOBS="2",
    )
    env["CGO_ENABLED"] = "0"
    for key in ("GOOS", "GOARCH", "GOARM", "GOAMD64", "GOMIPS", "GOMIPS64", "GOPPC64", "GOWASM"):
        env.pop(key, None)
    go_path = shutil.which("go", path=env.get("PATH", ""))
    if not go_path:
        raise RuntimeError("Go executable is not available on PATH")
    go_version = subprocess.check_output([go_path, "version"], cwd=ROOT, env=env, text=True).strip()
    (OUT / "go-version.log").write_text(go_version + "\n")
    if GO_TOOLCHAIN not in go_version.split():
        raise RuntimeError(f"Go launcher selected {go_version!r}, want {GO_TOOLCHAIN}")
    oracle.require_commit_sha(PIN, "oracle commit")
    subprocess.run(["git", "cat-file", "-e", f"{PIN}^{{commit}}"], cwd=ROOT, check=True)
    suffix = ".exe" if os.name == "nt" else ""
    rows = []

    def run(name, cmd, cwd=ROOT):
        with (OUT / (name + ".log")).open("wb") as log:
            result = subprocess.run(
                cmd,
                cwd=cwd,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                timeout=900,
            )
        rows.append(dict(name=name, argv=cmd, exit=result.returncode))
        (OUT / "commands.json").write_text(json.dumps(dict(head=head, commands=rows), indent=2))
        print(name, result.returncode, flush=True)
        if result.returncode:
            raise RuntimeError((OUT / (name + ".log")).read_text(errors="replace")[-6000:])

    meta = json.loads(
        subprocess.check_output(
            [
                "cargo",
                "metadata",
                "--manifest-path",
                str(ROOT / "Cargo.toml"),
                "--no-deps",
                "--format-version",
                "1",
            ],
            cwd=ROOT,
            env=env,
        )
    )
    assert Path(meta["workspace_root"]).resolve() == ROOT
    # Source archive is a trusted immutable Git object, not ambient checkout Go.
    archive = subprocess.check_output(["git", "archive", "--format=tar", PIN], cwd=ROOT)
    with tempfile.TemporaryDirectory(prefix="guard-go-", dir=OUT) as temp:
        source = Path(temp)
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
            tar.extractall(source, filter="data")
        source_identity = oracle.provenance(source, expected_commit_sha=PIN)
        source_files = verify_source_files(source)
        go = OUT / ("symbrain-go" + suffix)
        run("go-build", [go_path, "build", "-o", str(go), "./cmd/symbrain"], source)
        oracle.GO = Path(go_path).resolve()
        toolchain = oracle.toolchain_metadata(go, env)
        if toolchain["selected_version"] != GO_TOOLCHAIN:
            raise RuntimeError(
                f"Go binary selected {toolchain['selected_version']!r}, want {GO_TOOLCHAIN}"
            )
        (OUT / "go-toolchain.json").write_text(json.dumps(toolchain, indent=2, sort_keys=True) + "\n")
        run(
            "rust-build",
            [
                "cargo",
                "build",
                "--manifest-path",
                str(ROOT / "Cargo.toml"),
                "--locked",
                "-p",
                "symbrain-cli",
            ],
        )
        rust = OUT / "target/debug" / ("symbrain" + suffix)
        run(
            "rust-trust-anchor",
            [
                "cargo",
                "test",
                "--manifest-path",
                str(ROOT / "Cargo.toml"),
                "--locked",
                "-p",
                "symbrain-guard-core",
                "--test",
                "external_repair",
            ],
        )
        run(
            "fixture-mutation-controls",
            [sys.executable, str(ROOT / "guard/scripts/guard-decide-oracle/test_repair_parity.py")],
        )
        run(
            "differential",
            [
                sys.executable,
                str(GENERATOR),
                "--go",
                str(go),
                "--rust",
                str(rust),
                "--report",
                str(OUT / "differential.json"),
                "--fixture",
                str(FIXTURE),
                "--check-fixture",
                "--source-root",
                str(source),
                "--validation-script",
                str(VALIDATOR),
                "--oracle-commit",
                PIN,
                "--go-toolchain",
                GO_TOOLCHAIN,
                "--generator-sha256",
                ANCHOR.generator_sha256,
                "--validation-basis-sha256",
                ANCHOR.validator_sha256,
            ],
        )
        run(
            "regressions",
            [
                "cargo",
                "test",
                "--manifest-path",
                str(ROOT / "Cargo.toml"),
                "--locked",
                "-p",
                "symbrain-guard-core",
                "--test",
                "external_repair",
            ],
        )
        run(
            "raw-byte-production",
            [
                "cargo",
                "test",
                "--manifest-path",
                str(ROOT / "Cargo.toml"),
                "--locked",
                "-p",
                "symbrain-cli",
                "--test",
                "guard_decide_raw_bytes",
            ],
        )
        run(
            "adapter",
            [
                "cargo",
                "test",
                "--manifest-path",
                str(ROOT / "Cargo.toml"),
                "--locked",
                "-p",
                "symbrain-cli",
                "--test",
                "guard_decide_adapter",
            ],
        )
        if not os.environ.get("GUARD_REPAIR_SKIP_COORDINATED"):
            run(
                "coordinated-provenance-controls",
                [
                    sys.executable,
                    str(ROOT / "guard/scripts/guard-decide-oracle/test_coordinated_provenance.py"),
                ],
            )
        (OUT / "identity.json").write_text(
            json.dumps(
                {
                    "head": head,
                    "oracle_commit": PIN,
                    "source_tree_verified": bool(
                        source_identity["source_before"] == source_identity["source_after"]
                    ),
                    "source_files": source_files,
                    "go_toolchain": GO_TOOLCHAIN,
                    "go_version": go_version,
                    "effective_toolchain": toolchain,
                    "generator_sha256": file_sha256(GENERATOR, normalize_crlf=True),
                    "validation_basis_sha256": file_sha256(VALIDATOR, normalize_crlf=True),
                    "fixture_sha256": file_sha256(FIXTURE),
                    "binary_sha256": {
                        str(p.name): file_sha256(p) for p in [go, rust]
                    },
                },
                indent=2,
                sort_keys=True,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()
