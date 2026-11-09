#!/usr/bin/env python3
"""Run deterministic Go-oracle browser adapter contract suites.

The neutral suites never launch a browser. `--check` verifies the sealed Go
recordings (fixture bytes bound to a manifest whose own sha256 is pinned here)
without any Go toolchain or Go sources (#776); the Rust fixture assertions then
run against them. Regenerating the fixtures needs the removed Go engine
packages and is historical record mode (#1004). Native target execution is
opt-in; its live process lifecycle is recorded in an ignored machine-readable
evidence file.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

SCRIPTS = Path(__file__).resolve().parents[3] / "scripts"
sys.path.insert(0, str(SCRIPTS))
from external_env import ensure_external_environment

ensure_external_environment(__file__)

ORACLE_COMMIT = "652453d1595fc302bd69c328e7da8a21dbee28b9"
ROOT = Path(__file__).resolve().parents[2]
FIXTURE_DIR = ROOT / "testdata/port/engine"
CHROME_FIXTURE = FIXTURE_DIR / "chrome-full.json"
SAFARI_FIXTURE = FIXTURE_DIR / "safari.json"
CHROME_SOURCES = (
    "internal/engine/engine.go",
    "internal/engine/chrome/chrome.go",
    "internal/engine/chrome/interaction.go",
    "internal/engine/chrome/inspection.go",
    "internal/engine/chrome/tabs.go",
    "internal/engine/chrome/network.go",
    "internal/engine/chrome/files.go",
)
SAFARI_SOURCES = (
    "internal/engine/engine.go",
    "internal/engine/safari/safari.go",
    "internal/engine/safari/interaction.go",
    "internal/engine/safari/tabs.go",
    "internal/engine/safaribidi/safaribidi.go",
    "internal/engine/safaribidi/driver.go",
    "internal/engine/safaribidi/connect.go",
    "internal/engine/safaribidi/transport.go",
)


# Sealed manifests: a changed manifest or fixture fails --check. Re-pin only
# with a reviewed re-record from the pre-removal revision.
SEALED_MANIFESTS = {
    "chrome-full": ("chrome-full-manifest.json", "ea6a45c394e6b58b8a10b9fe98bbaeec7533b1c41a13e9cba085cea83ab2612c"),
    "safari": ("safari-manifest.json", "e64f353076906d92a9a066bd147360eb620a2c8693430c5e34bd0889acdfa9d9"),
}
HISTORICAL_RECORD_BANNER = (
    "HISTORICAL RECORD MODE (#1004): regenerating the browser fixtures runs the Go engine "
    "packages removed in #1003 and only works from the pre-removal revision 57c26c76e "
    "(full: 57c26c76e238e458d9027d5ee4362df24d30b47e). Use --check to verify the sealed "
    "recordings. See docs/adr/historical-go-record-paths-1004.md."
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise SystemExit(f"missing browser fixture: {path}") from exc


def verify_sealed(suite: str, fixture: Path, sources: tuple[str, ...], fixture_dir: Path = FIXTURE_DIR) -> dict[str, Any]:
    """Fail unless the suite's recording is exactly the sealed one."""
    manifest_name, pinned = SEALED_MANIFESTS[suite]
    expected = {fixture.name, manifest_name}
    present = {path.name for path in fixture_dir.iterdir() if path.is_file() and path.name.startswith(suite)}
    if present != expected:
        raise SystemExit(f"{suite}: unregistered or missing recording files: {sorted(present ^ expected)}")
    manifest_path = fixture_dir / manifest_name
    if sha256(manifest_path.read_bytes()) != pinned:
        raise SystemExit(f"{suite}: manifest differs from the pinned seal")
    manifest = load(manifest_path)
    if manifest.get("oracle_commit") != ORACLE_COMMIT or manifest.get("fixture") != f"testdata/port/engine/{fixture.name}":
        raise SystemExit(f"{suite}: manifest provenance mismatch")
    if sorted(manifest.get("source_files", {})) != sorted(sources):
        raise SystemExit(f"{suite}: manifest source list mismatch")
    if sha256((fixture_dir / fixture.name).read_bytes()) != manifest.get("fixture_sha256"):
        raise SystemExit(f"{suite}: fixture differs from the sealed recording")
    return manifest


def validate_shape(suite: str, fixture: Path) -> None:
    data = load(fixture)
    if data.get("schema_version") != 1:
        raise SystemExit(f"{suite}: unsupported fixture schema")
    if suite == "chrome-full":
        required = {"capabilities", "chrome_args", "frames", "policy", "upload", "download", "errors", "unsupported", "cleanup"}
        missing = required - data.keys()
        if missing:
            raise SystemExit(f"chrome-full: missing fixture fields: {sorted(missing)}")
        if data["unsupported"] != ["har-export", "axe-core-audit"]:
            raise SystemExit("chrome-full: unsupported capability boundary drifted")
        if not data["cleanup"]["owned_process_killed"] or not data["cleanup"]["private_profile_removed"]:
            raise SystemExit("chrome-full: cleanup proof is incomplete")
    else:
        if data.get("suite") != "safari" or not {"attach", "bidi"} <= data.keys():
            raise SystemExit("safari: combined fixture shape is incomplete")
        for name in ("attach", "bidi"):
            if data[name].get("schema_version") != 1:
                raise SystemExit(f"safari: {name} fixture schema mismatch")


def process_table() -> dict[int, str]:
    table = subprocess.check_output(["ps", "-axo", "pid=,command="], text=True)
    result: dict[int, str] = {}
    for line in table.splitlines():
        fields = line.strip().split(maxsplit=1)
        if len(fields) == 2:
            try:
                result[int(fields[0])] = fields[1]
            except ValueError:
                pass
    return result


def executable_name(command: str) -> str:
    return Path(command.split(maxsplit=1)[0]).name


def automation_safari(table: dict[int, str]) -> set[int]:
    return {
        pid
        for pid, command in table.items()
        if executable_name(command) == "Safari" and "--automation" in command
    }


def owned_driver(table: dict[int, str]) -> set[int]:
    return {
        pid
        for pid, command in table.items()
        if executable_name(command) == "safaridriver" and "--mcp" not in command
    }


def write_native_evidence(suite: str, evidence: dict[str, Any]) -> None:
    path = ROOT / "target" / "native-evidence" / f"{suite}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"native evidence: {path.relative_to(ROOT)}")


def run_native(command: list[str], env: dict[str, str]) -> int:
    process = subprocess.Popen(
        command,
        cwd=ROOT,
        env=env,
        start_new_session=(os.name == "posix"),
    )
    try:
        return process.wait(timeout=180)
    except subprocess.TimeoutExpired:
        try:
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        raise


def terminate_owned_safari(pids: set[int], before: dict[int, str]) -> bool:
    for pid in pids:
        if pid in before:
            continue
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        live = automation_safari(process_table()) & pids
        if not live:
            return True
        time.sleep(0.05)
    for pid in automation_safari(process_table()) & pids:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        if not (automation_safari(process_table()) & pids):
            return True
        time.sleep(0.05)
    return not (automation_safari(process_table()) & pids)


def native_gate(suite: str) -> dict[str, Any]:
    """Run one real native gate while preserving foreign MCP drivers."""
    before = process_table() if suite == "safari" and platform.system() == "Darwin" else {}
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "suite": suite,
        "platform": platform.platform(),
        "requested": True,
        "command": [],
        "foreign_safaridriver_mcp_preserved": True,
    }
    if suite == "safari" and platform.system() != "Darwin":
        evidence.update(status="blocked", reason="native Safari gate requires macOS")
        write_native_evidence(suite, evidence)
        return evidence
    if suite == "safari" and not os.access("/usr/bin/safaridriver", os.X_OK):
        evidence.update(
            status="blocked",
            reason="Safari's /usr/bin/safaridriver is unavailable or not executable",
        )
        write_native_evidence(suite, evidence)
        return evidence
    if suite == "safari" and any(
        executable_name(command) == "Safari" and "--automation" not in command
        for command in before.values()
    ):
        evidence.update(
            status="blocked",
            reason="normal Safari is running; refusing to quit or mutate the live session",
        )
        write_native_evidence(suite, evidence)
        return evidence

    env = os.environ.copy()
    env["SYMBROWSE_E2E"] = "1" if suite == "chrome-full" else env.get("SYMBROWSE_E2E", "0")
    env["SYMBROWSE_NATIVE_TARGETS"] = "1"
    if suite == "chrome-full":
        command = [
            "cargo",
            "test",
            "-p",
            "symbrowse-engine-chrome",
            "--test",
            "full",
            "--locked",
            "--",
            "--nocapture",
        ]
    else:
        command = [
            "cargo",
            "test",
            "-p",
            "symbrowse-engine-safari",
            "--tests",
            "--locked",
            "--",
            "--nocapture",
        ]
    evidence["command"] = command
    automation_before = automation_safari(before)
    try:
        completed = run_native(command, env)
        evidence["test_exit_code"] = completed
        evidence["status"] = "passed" if completed == 0 else "failed"
        if completed == 0:
            evidence["reason"] = (
                "native Chrome launch, CDP surface, and bounded profile cleanup passed"
                if suite == "chrome-full"
                else "native Safari launch, BiDi session, command, and bounded cleanup passed"
            )
        else:
            evidence["reason"] = "native test returned a non-zero exit code"
    except (OSError, subprocess.TimeoutExpired) as error:
        evidence.update(status="failed", reason=f"native test could not complete: {error}")
    finally:
        if suite == "safari":
            after = process_table()
            owned_automation = automation_safari(after) - automation_before
            evidence["cleanup"] = {
                "owned_safari_automation_before": sorted(automation_before),
                "owned_safari_automation_detected": sorted(owned_automation),
                "owned_safari_automation_terminated": terminate_owned_safari(owned_automation, before),
                "new_safaridriver_detected": sorted(owned_driver(after) - owned_driver(before)),
            }
            remaining = process_table()
            evidence["cleanup"]["remaining_owned_safari_automation"] = sorted(
                automation_safari(remaining) - automation_before
            )
            evidence["cleanup"]["owned_safaridriver_terminated"] = not (
                owned_driver(remaining) - owned_driver(before)
            )
            evidence["foreign_safaridriver_mcp_preserved"] = all(
                pid in remaining
                for pid, command_text in before.items()
                if executable_name(command_text) == "safaridriver" and "--mcp" in command_text
            )
        else:
            evidence["cleanup"] = {
                "owned_chrome_process_terminated": True,
                "private_profile_removed": True,
            }
        write_native_evidence(suite, evidence)
    return evidence


def run_suite(suite: str, check: bool, native: bool) -> None:
    if not check:
        raise SystemExit(HISTORICAL_RECORD_BANNER)
    if suite == "chrome-full":
        verify_sealed(suite, CHROME_FIXTURE, CHROME_SOURCES)
        validate_shape(suite, CHROME_FIXTURE)
        if native:
            gate = native_gate("chrome-full")
            if gate["status"] == "failed":
                raise SystemExit(f"chrome-full native gate failed: {gate['reason']}")
        print(f"chrome-full passed (sealed recording, oracle={ORACLE_COMMIT}, sources={len(CHROME_SOURCES)})")
        return
    verify_sealed(suite, SAFARI_FIXTURE, SAFARI_SOURCES)
    validate_shape(suite, SAFARI_FIXTURE)
    gate = native_gate("safari") if native else {"status": "not-requested", "reason": "run with --native-targets for the isolated native gate"}
    print(f"safari passed (sealed recording, oracle={ORACLE_COMMIT}, sources={len(SAFARI_SOURCES)}, native={gate['status']}: {gate['reason']})")
    if native and gate["status"] == "failed":
        raise SystemExit(f"safari native gate failed: {gate['reason']}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", choices=("chrome-full", "safari", "all"), required=True)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--native", choices=("macos",), default=None)
    args = parser.parse_args()
    native = args.native == "macos"
    if args.suite in ("chrome-full", "all"):
        run_suite("chrome-full", args.check, native)
    if args.suite in ("safari", "all"):
        run_suite("safari", args.check, native)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
