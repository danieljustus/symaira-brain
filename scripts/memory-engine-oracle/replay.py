#!/usr/bin/env python3
"""Prepared real-process package comparison; never builds or contacts an LLM."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
sys.dont_write_bytecode = True

import binding as proof


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv, cwd, env, label):
    try:
        result = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, timeout=20)
        status = result.returncode
        stdout, stderr = result.stdout, result.stderr
        timed_out = False
    except subprocess.TimeoutExpired as error:
        status = None
        stdout, stderr = error.stdout or b"", error.stderr or b""
        timed_out = True
    (cwd / (label + ".stdout")).write_bytes(stdout)
    (cwd / (label + ".stderr")).write_bytes(stderr)
    return {"argv": argv, "status": status, "timed_out": timed_out,
            "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
            "stderr_sha256": hashlib.sha256(stderr).hexdigest()}


def records(path, expected):
    value = json.loads(path.read_bytes())
    if not isinstance(value, list) or len(value) != len(expected) or not value:
        raise ValueError("missing, additional or zero result records")
    if [(v.get("id"), v.get("operation")) for v in value] != [
            (v["id"], v["operation"]) for v in expected]:
        raise ValueError("result identities or order changed")
    return value


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go-test", type=Path, required=True)
    parser.add_argument("--rust-probe", type=Path, required=True)
    parser.add_argument("--binding", type=Path, required=True,
                        help="source/SDK/dependency/helper/binary manifest from actual builds")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--go-source", type=Path, required=True)
    parser.add_argument("--rust-source", type=Path, required=True)
    parser.add_argument("--go-sdk", type=Path, required=True)
    parser.add_argument("--rust-sdk", type=Path, required=True)
    parser.add_argument("--control", choices=["changed-input", "zero-selected", "output-obstruction", "omitted-case"])
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    here = Path(__file__).resolve().parent
    args.binding = args.binding.resolve()
    try:
        cases = proof.cases()
        binding = json.loads(args.binding.read_bytes())
        binaries = {"go": args.go_test.resolve(strict=True),
                    "rust": args.rust_probe.resolve(strict=True)}
        before = proof.snapshot(args.go_source, args.rust_source, args.go_sdk, args.rust_sdk, binaries)
        proof.verify_receipt(binding, before, args.binding)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        rejection = {"status": "FAIL", "admission_error": str(error),
                     "runner_sha256": digest(Path(__file__)), "product_processes": 0}
        (args.output / "receipt.json").write_text(json.dumps(rejection, indent=2) + "\n")
        print(json.dumps(rejection))
        return 1
    result = {"status": "FAIL", "cases": len(cases), "control": args.control,
              "cases_sha256": digest(here / "cases.json"),
              "runner_sha256": digest(Path(__file__)),
              "binding": binding, "binding_sha256": digest(args.binding),
              "before": before}
    with tempfile.TemporaryDirectory(prefix="owned-", dir=args.output) as temporary:
        work = Path(temporary)
        for part in ["home", "config", "data", "cache", "tmp"]:
            (work / part).mkdir()
        env = {"HOME": str(work / "home"), "USERPROFILE": str(work / "home"),
               "XDG_CONFIG_HOME": str(work / "config"), "XDG_DATA_HOME": str(work / "data"),
               "XDG_CACHE_HOME": str(work / "cache"), "TMPDIR": str(work / "tmp"),
               "TMP": str(work / "tmp"), "TEMP": str(work / "tmp"), "PATH": ""}
        for key in ["SYSTEMROOT", "WINDIR"]:
            if key in os.environ:
                env[key] = os.environ[key]
        go_input, rust_input = work / "go-input.json", work / "rust-input.json"
        go_input.write_text(json.dumps(cases), encoding="utf-8")
        rust_cases = json.loads(json.dumps(cases))
        if args.control == "changed-input":
            rust_cases[0]["text_hex"] = b"plain greeting without a trigger".hex()
        if args.control == "omitted-case":
            rust_cases.pop()
        rust_input.write_text(json.dumps(rust_cases), encoding="utf-8")
        go_output, rust_output = work / "go-results.json", work / "rust-results.json"
        if args.control == "output-obstruction":
            rust_output.mkdir()
        env["SYMBRAIN_ENGINE_ORACLE_INPUT"] = str(go_input)
        env["SYMBRAIN_ENGINE_ORACLE_OUTPUT"] = str(go_output)
        selector = "^NoSuchOwnedOracleTest$" if args.control == "zero-selected" else "^TestNativeMemoryEngineOracle$"
        result["environment"] = env
        result["go"] = run([str(binaries["go"]), "-test.run=" + selector, "-test.v"], work, env, "go")
        if {role: digest(path) for role, path in binaries.items()} != before["binaries"]:
            result["executable_change_between_runs"] = True
            result["rust"] = {"status": None, "skipped": "executable identity changed after oracle"}
        else:
            result["rust"] = run([str(binaries["rust"]), str(rust_input), str(rust_output)], work, env, "rust")
        for path in [go_input, rust_input, go_output, rust_output,
                     work / "go.stdout", work / "go.stderr", work / "rust.stdout", work / "rust.stderr"]:
            if path.is_file():
                (args.output / path.name).write_bytes(path.read_bytes())
        failures = []
        if result["go"]["status"] != 0 or result["rust"]["status"] != 0:
            failures.append("actual executable failed or timed out")
        try:
            go = records(go_output, cases)
            rust = records(rust_output, cases)
            failures.extend(case["id"] for case, left, right in zip(cases, go, rust, strict=True)
                            if left != right)
        except (ValueError, OSError) as error:
            failures.append(str(error))
        result["failures"] = failures
        result["status"] = "PASS" if not failures else "FAIL"
    try:
        result["after"] = proof.snapshot(args.go_source, args.rust_source, args.go_sdk, args.rust_sdk, binaries)
        proof.verify_receipt(binding, result["after"], args.binding)
        if result["after"] != before or result.get("executable_change_between_runs"):
            failures.append("source/input/SDK/executable identity changed during actual processes")
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        failures.append("post-process binding failed: " + str(error))
    result["status"] = "PASS" if not failures else "FAIL"
    # Preserve source/binary maps and all failures; no runtime receipt is reused.
    (args.output / "receipt.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"status": result["status"], "cases": len(cases), "failures": result["failures"]}))
    return int(bool(failures))


if __name__ == "__main__":
    sys.exit(main())
