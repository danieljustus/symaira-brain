#!/usr/bin/env python3
"""Compare native ``symbrain guard grants`` with the frozen Go fixture."""
from __future__ import annotations

import argparse
import base64
import json
import os
from pathlib import Path
import platform
import tempfile

import oracle


def validate_encoded(stream: dict[str, object], label: str) -> None:
    data = base64.b64decode(stream["base64"])
    if stream["bytes"] != len(data) or stream["sha256"] != oracle.sha256(data):
        raise RuntimeError(f"fixture contains invalid byte metadata for {label}")


def compare(binary: Path, fixture: dict[str, object]) -> list[str]:
    failures: list[str] = []
    with tempfile.TemporaryDirectory(
        prefix="guard-grants-parity-",
        dir=os.environ.get("SYMAIRA_EXTERNAL_RUNTIME_ROOT"),
    ) as temporary:
        runtime = Path(temporary)
        for case in fixture["cases"]:
            case_id = case["id"]
            try:
                validate_encoded(case["stdout"], f"{case_id} stdout")
                validate_encoded(case["stderr"], f"{case_id} stderr")
                for item in case["state"]["files"]:
                    validate_encoded(item["content"], f"{case_id} {item['path']}")
                actual = oracle.run_case(
                    binary, oracle.ROOT, runtime, case_id, list(case["args"])
                )
                for stream in ("stdout", "stderr"):
                    if actual[stream] != case[stream]:
                        failures.append(
                            f"{case_id} {stream}: expected {case[stream]['sha256']}, "
                            f"got {actual[stream]['sha256']}"
                        )
                if actual["exit_code"] != case["exit_code"]:
                    failures.append(
                        f"{case_id} exit_code: expected {case['exit_code']}, "
                        f"got {actual['exit_code']}"
                    )
                if actual["state"] != case["state"]:
                    failures.append(f"{case_id} state: native state differs (mode/content included)")
                    if actual["state"]["directories"] != case["state"]["directories"]:
                        failures.append(
                            f"{case_id} state directories: expected {case['state']['directories']}, "
                            f"got {actual['state']['directories']}"
                        )
                    expected_files = {item["path"]: item for item in case["state"]["files"]}
                    actual_files = {item["path"]: item for item in actual["state"]["files"]}
                    for path in sorted(set(expected_files) | set(actual_files)):
                        expected_item = expected_files.get(path)
                        actual_item = actual_files.get(path)
                        if expected_item is None or actual_item is None:
                            failures.append(f"{case_id} state file {path}: missing or unexpected")
                            continue
                        if expected_item["mode"] != actual_item["mode"]:
                            failures.append(
                                f"{case_id} state file {path} mode: expected "
                                f"{expected_item['mode']}, got {actual_item['mode']}"
                            )
                        if expected_item["content"] != actual_item["content"]:
                            failures.append(
                                f"{case_id} state file {path} content: expected "
                                f"{expected_item['content']['sha256']}, got "
                                f"{actual_item['content']['sha256']}"
                            )
            except Exception as error:  # keep every case result visible
                failures.append(f"{case_id}: {error}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path, help="path to the native symbrain binary")
    args = parser.parse_args()
    if platform.system() == "Darwin" and os.environ.get("SYMAIRA_EXTERNAL_ENV_READY") != "1":
        raise RuntimeError("invoke through bash scripts/run-external-env.sh")
    binary = args.binary.expanduser().resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise RuntimeError(f"native binary is not executable: {binary}")
    fixture = json.loads(oracle.FIXTURE.read_text(encoding="utf-8"))
    oracle.verify_binding(fixture)
    if fixture.get("case_count") != len(fixture.get("cases", [])):
        raise RuntimeError("fixture case_count does not match grants cases")
    failures = compare(binary, fixture)
    if failures:
        raise RuntimeError("native/Go grants parity failed:\n" + "\n".join(f"  {failure}" for failure in failures))
    print(f"PASS: native/Go guard grants parity passed ({fixture['case_count']} cases)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, json.JSONDecodeError) as error:
        print(f"guard-grants-parity: {error}", file=os.sys.stderr)
        raise SystemExit(1)
