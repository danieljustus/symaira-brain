#!/usr/bin/env python3
"""Allocated-runtime builder; never used by source-only preparation."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import binding


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("go-source", "rust-source", "go-sdk", "rust-sdk", "target", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    for name in ("go_source", "rust_source", "go_sdk", "rust_sdk", "target", "output"):
        setattr(args, name, getattr(args, name).resolve())
    args.output.mkdir(parents=True, exist_ok=False)
    # The owner supplies an exclusive, existing Cargo target; never select a
    # target implicitly or build before checking independently frozen inputs.
    if not args.target.is_dir():
        raise ValueError("allocated existing target required")
    binding.cases()
    source_before = binding.sources(args.go_source, args.rust_source)
    sdk_before = binding.sdks(args.go_sdk, args.rust_sdk)
    for name in ("home", "tmp", "go-cache"):
        (args.output / name).mkdir()
    inherited = {name: os.environ[name] for name in ("PATH", "CARGO_HOME", "RUSTUP_HOME", "GOMODCACHE", "SystemRoot", "SYSTEMROOT", "WINDIR") if name in os.environ}
    environment = dict(inherited, HOME=str(args.output / "home"),
                       USERPROFILE=str(args.output / "home"), TMPDIR=str(args.output / "tmp"),
                       TMP=str(args.output / "tmp"), TEMP=str(args.output / "tmp"),
                       GOCACHE=str(args.output / "go-cache"), GOTOOLCHAIN="local",
                       GOTELEMETRY="off", GOENV="off", GOFLAGS="", GOPROXY="off",
                       RUSTC=str(args.rust_sdk / "bin/rustc"), RUSTDOC=str(args.rust_sdk / "bin/rustdoc"),
                       RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER="",
                       CGO_ENABLED="0", CARGO_TARGET_DIR=str(args.target.resolve()),
                       CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0",
                       CARGO_PROFILE_TEST_DEBUG="0")
    environment["PATH"] = str(args.rust_sdk / "bin") + os.pathsep + str(args.go_sdk / "bin") + os.pathsep + environment.get("PATH", "")
    go = args.output / ("engine-go.exe" if os.name == "nt" else "engine-go")
    rust = args.target.resolve() / "debug/examples" / ("engine_probe.exe" if os.name == "nt" else "engine_probe")
    binaries = {"go": go, "rust": rust}
    commands = [("go", args.go_source, [str(args.go_sdk / "bin/go"), "test", "-p=2", "-mod=readonly", "-c", "-o", str(go), "./internal/memory/contextassembler"]),
                ("rust", args.rust_source, [str(args.rust_sdk / "bin/cargo"), "build", "--locked", "--offline", "-p", "symbrain-memory", "--example", "engine_probe"])]
    receipt = {"kind": "actual-memory760-build-v1", "builder_sha256": binding.digest(Path(__file__)), "steps": []}
    path = args.output / "build-receipt.json"
    for role, cwd, argv in commands:
        # Retain failed builds, never represent partial builds as acceptance.
        try:
            result = subprocess.run(argv, cwd=cwd, env=environment, capture_output=True, timeout=1800)
            exit_code, out, err = result.returncode, result.stdout, result.stderr
        except subprocess.TimeoutExpired as error:
            exit_code, out, err = None, error.stdout or b"", error.stderr or b""
        stdout, stderr = args.output / (role + ".build.stdout"), args.output / (role + ".build.stderr")
        stdout.write_bytes(out); stderr.write_bytes(err)
        receipt["steps"].append({"role": role, "argv": argv, "cwd": str(cwd), "exit": exit_code,
                                 "stdout_sha256": binding.digest(stdout), "stderr_sha256": binding.digest(stderr)})
        path.write_text(json.dumps(receipt, indent=2) + "\n")
        if exit_code != 0:
            return 1
    snapshot = binding.snapshot(args.go_source, args.rust_source, args.go_sdk, args.rust_sdk, binaries)
    if snapshot["sources"] != source_before or snapshot["sdks"] != sdk_before:
        raise ValueError("source or toolchain changed during actual build")
    receipt["snapshot"] = snapshot
    receipt["environment"] = environment
    path.write_text(json.dumps(receipt, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
