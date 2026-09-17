#!/usr/bin/env python3
"""Compare native guard scan output with the frozen Go oracle fixture."""
from __future__ import annotations

import argparse
import base64
import json
import os
from pathlib import Path
import platform
import tempfile

import oracle


def expected_bytes(stream: dict[str, object]) -> bytes:
    data = base64.b64decode(stream["base64"])
    if stream["bytes"] != len(data) or stream["sha256"] != oracle.sha256(data):
        raise RuntimeError("fixture contains invalid byte metadata")
    return data


def case_options(case_id: str) -> dict[str, object]:
    if case_id == "one-finding-missing-hermes":
        return {"missing_hermes": True, "unknown_opencode": False, "repeat": 3}
    return {}


def compare(binary: Path, fixture: dict[str, object]) -> list[str]:
    failures: list[str] = []
    with tempfile.TemporaryDirectory(
        prefix="guard-scan-parity-",
        dir=os.environ.get("SYMAIRA_EXTERNAL_RUNTIME_ROOT"),
    ) as temporary:
        base = Path(temporary)
        for case in fixture["cases"]:
            case_id = case["id"]
            options = case_options(case_id)
            root = base / case_id
            try:
                actual = oracle.run_scan_case(
                    binary,
                    oracle.ROOT,
                    root,
                    case_id,
                    list(case["args"]),
                    tty_mode=bool(case.get("tty", False)),
                    repeat=int(case.get("repeat", options.get("repeat", 1))),
                    **{key: value for key, value in options.items() if key != "repeat"},
                )
                expected = {
                    "exit_code": case["exit_code"],
                    "stdout": {
                        **case["stdout"],
                        "_data": expected_bytes(case["stdout"]),
                    },
                    "stderr": {
                        **case["stderr"],
                        "_data": expected_bytes(case["stderr"]),
                    },
                }
                for stream in ("stdout", "stderr"):
                    if actual[stream] != {key: value for key, value in expected[stream].items() if key != "_data"}:
                        failures.append(
                            f"{case_id} {stream}: expected {expected[stream]['sha256']}, "
                            f"got {actual[stream]['sha256']}"
                        )
                if actual["exit_code"] != expected["exit_code"]:
                    failures.append(
                        f"{case_id} exit_code: expected {expected['exit_code']}, got {actual['exit_code']}"
                    )
            except Exception as error:  # keep all nine case results visible
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
    if fixture.get("case_count") != 9 or len(fixture.get("cases", [])) != 9:
        raise RuntimeError("fixture must contain exactly 9 scan cases")
    failures = compare(binary, fixture)
    if failures:
        raise RuntimeError("native/Go scan parity failed:\n" + "\n".join(f"  {failure}" for failure in failures))
    print(f"PASS: native/Go guard scan parity passed ({fixture['case_count']} cases)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, json.JSONDecodeError) as error:
        print(f"guard-scan-parity: {error}", file=os.sys.stderr)
        raise SystemExit(1)
