#!/usr/bin/env python3
"""Allocated-runtime builder; never used by source-only preparation."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
sys.dont_write_bytecode = True
import binding
import dependencies
import owner


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    names = ("go-source", "rust-source", "go-sdk", "rust-sdk", "go-modcache", "cargo-cache", "target", "output")
    for name in names:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    for name in names:
        attribute = name.replace("-", "_")
        setattr(args, attribute, getattr(args, attribute).resolve())
    args.output.mkdir(parents=True, exist_ok=False)
    if not args.target.is_dir() or any(args.target.iterdir()):
        raise ValueError("allocated empty target required; unbound derived artifacts cannot be reused")
    if any(args.target.is_relative_to(root) or args.output.is_relative_to(root) for root in (args.go_source, args.rust_source)):
        raise ValueError("target/output must be outside compilation checkouts")
    receipt = {"kind": "actual-memory760-build-v2", "builder_sha256": binding.digest(Path(__file__)), "steps": []}
    path = args.output / "build-receipt.json"
    try:
        binding.cases()
        before = binding.sources(args.go_source, args.rust_source)
        sdk_before = binding.sdks(args.go_sdk, args.rust_sdk)
        inputs = dependencies.admit(args.go_source, args.rust_source, args.go_modcache, args.cargo_cache)
        for name in ("home", "tmp", "go-cache"):
            (args.output / name).mkdir()
        (args.output / "empty-git-config").write_bytes(b"")
        owned = {"go": args.output / "go-source", "rust": args.output / "rust-source"}
        owner.stage(args.go_source, before["owned_go"], owned["go"])
        owner.stage(args.rust_source, before["rust"], owned["rust"])
        dependencies.materialize(args.go_modcache, inputs["go"], args.output / "go-modcache")
        dependencies.materialize(args.cargo_cache, inputs["rust"], args.output / "cargo-home")
        env = owner.environment(args.output, args.go_sdk, args.rust_sdk, args.target)
        receipt.update(environment=env, target=str(args.target),
                       owned_checkouts={role: str(root) for role, root in owned.items()},
                       dependency_roots={"go": str(args.go_modcache), "rust": str(args.cargo_cache)}, dependency_inputs=inputs)
        go = args.output / ("engine-go.exe" if os.name == "nt" else "engine-go")
        binaries = {"go": go}
        commands = [("go", [str(args.go_sdk / "bin/go"), "test", "-p=2", "-mod=readonly", "-c", "-o", str(go), "./internal/memory/contextassembler"]),
                    ("rust", [str(args.rust_sdk / "bin/cargo"), "build", "--locked", "--offline", "-p", "symbrain-memory", "--example", "engine_probe", "--message-format=json"])]
        for role, argv in commands:
            # Admission and staging are complete before the first SDK child.
            # Failed/partial builds retain every stream and never become proof.
            try:
                result = subprocess.run(argv, cwd=owned[role], env=env, capture_output=True, timeout=1800)
                code, out, err = result.returncode, result.stdout, result.stderr
            except subprocess.TimeoutExpired as error:
                code, out, err = None, error.stdout or b"", error.stderr or b""
            stdout, stderr = args.output / (role + ".build.stdout"), args.output / (role + ".build.stderr")
            stdout.write_bytes(out); stderr.write_bytes(err)
            receipt["steps"].append({"role": role, "argv": argv, "cwd": str(owned[role]), "exit": code,
                                     "stdout_sha256": binding.digest(stdout), "stderr_sha256": binding.digest(stderr)})
            path.write_text(json.dumps(receipt, indent=2) + "\n")
            if code != 0:
                return 1
            if role == "rust":
                binaries["rust"] = owner.artifact(out, args.target, owned["rust"] / "rust/symbrain-memory/Cargo.toml")
        snapshot = binding.snapshot(args.go_source, args.rust_source, args.go_sdk, args.rust_sdk, binaries)
        if snapshot["sources"] != before or snapshot["sdks"] != sdk_before:
            raise ValueError("source or toolchain changed during actual build")
        receipt["snapshot"] = snapshot
        path.write_text(json.dumps(receipt, indent=2) + "\n")
        binding.verify_receipt(receipt, snapshot, path)
        return 0
    except (ValueError, OSError) as error:
        receipt["admission_error"] = str(error)
        path.write_text(json.dumps(receipt, indent=2) + "\n")
        raise


if __name__ == "__main__":
    raise SystemExit(main())
