#!/usr/bin/env python3
"""Go-free state-key gate: replay the sealed Go recording against Rust (#772).

The historical gate (`daemon_state_key.py`) drove a pinned Go binary and Go
probe programs that were removed in #1003. Its last accepted run recorded every
Go observation next to the Rust one. This replay keeps the Go side fixed to
that sealed recording and observes only the Rust candidate, with the same case
inputs, the same normalization and the same rejecting controls.

Sealing: `retention.json` in the recording directory lists the sha256 of every
retained file, and its own sha256 is pinned below. A changed, missing or
unregistered recording file fails before any case runs.

Local macOS runs need a short `SYMAIRA_EXTERNAL_RUNTIME_ROOT`: the daemon
socket path must stay below the Unix sun_path limit.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile

import daemon_registry as registry
import daemon_state_key as state_key
import fixture_programs
import key_test_environment

RECORDING_DIR = state_key.ROOT / "migration/evidence/browse-state-key-772/candidate-4cd"
RECORDING = "symaira-state-key-final-gate.json"
RETENTION = "retention.json"
# Pins the sealed file list; re-pin only with a reviewed re-record.
RETENTION_SHA256 = "fbf53762daa143397a6ef677f8dba68f21868d26e41efd2f416aa1ea15d88e80"
CONTROLS = (("wrong-environment", "environment-ab"),
            ("wrong-vault", "vault-ab-environment-ab"),
            ("denied-to-absence", "vault-4-environment-ab"))
# Release builds back up pre-Rust state once on first run (symaira-cutover);
# the historical Go recording predates that notice, so the replay opts out.
REPLAY_ENV = {"SYMAIRA_CUTOVER_BACKUP": "off"}
KEYCHAIN_LOOKUP = ["find-generic-password", "-s", "symbrowse", "-a", "encryption-key", "-w"]
STORE_WARNING = ('WARN re-saving encrypted state without an encryption key '
                 'state="existing" previous_key_source="environment"')


class SealError(AssertionError):
    """The recording is not the sealed one."""


def verify_seal(directory: Path, retention_sha256: str = RETENTION_SHA256) -> dict:
    """Return the sealed recording or raise SealError naming every failure."""
    retention_path = directory / RETENTION
    if not retention_path.is_file():
        raise SealError(f"missing {RETENTION} in {directory}")
    if registry.process.digest(retention_path) != retention_sha256:
        raise SealError(f"{RETENTION} differs from the pinned seal")
    sealed = json.loads(retention_path.read_text())["files_sha256"]
    # Recursive: a nested or differently named file is unregistered too, and
    # symlinks (which rglob neither follows nor reports as files) are refused.
    links = [path for path in directory.rglob("*") if path.is_symlink()]
    if links:
        raise SealError(f"symlink in recording directory: {links[0].relative_to(directory).as_posix()}")
    present = {path.relative_to(directory).as_posix() for path in directory.rglob("*")
               if not path.is_dir()} - {RETENTION}
    failures = [f"unregistered recording file: {name}" for name in sorted(present - sealed.keys())]
    failures += [f"missing recording file: {name}" for name in sorted(sealed.keys() - present)]
    failures += [f"changed recording file: {name}" for name in sorted(present & sealed.keys())
                 if registry.process.digest(directory / name) != sealed[name]]
    if RECORDING not in sealed:
        failures.append(f"{RECORDING} is not part of the seal")
    if failures:
        raise SealError("\n".join(failures))
    return json.loads((directory / RECORDING).read_text())


def check_inputs(recording: dict) -> None:
    """The recording must describe exactly today's frozen inputs and cases."""
    assert recording["matches"] and all(row["matches"] for row in recording["cases"]), "recording was not a passing gate"
    assert recording["historical_manifest_sha256"] == registry.process.digest(state_key.FROZEN / "manifest.json")
    for fixture in state_key.MANIFEST["cases"]:
        assert registry.process.digest(state_key.FROZEN / fixture["path"]) == fixture["sha256"], fixture["path"]
    recorded = [row["input"] for row in recording["cases"]]
    # Keychain cases exist only on Darwin; the sealed recording is from Linux.
    current = [case for case in state_key.inputs() if "keychain" not in case]
    assert recorded == current, "case inputs drifted from the sealed recording"


def comparable(observation: dict) -> dict:
    """Normalize one side for the replay comparison.

    The session name embeds the observing harness PID, which differs between
    the recording and this run; it is replaced by a placeholder in request
    frames. On Darwin the owned keychain fixture always answers "item not
    found" (security exit 44), which the daemon treats exactly like the absent
    keychain of the Linux recording, so that lookup is not compared.
    """
    arguments = observation["arguments"]
    session = arguments[arguments.index("--session") + 1]
    value = state_key.normalized(observation)
    for record in value["records"]:
        if record["request"].get("session") == session:
            record["request"]["session"] = "<SESSION>"
    if platform.system() == "Darwin":
        keychain = [query for query in value["queries"] if query["name"] == "security"]
        # At most one lookup, and only of the daemon's own key item.
        if len(keychain) > 1 or any(query["arguments"] != KEYCHAIN_LOOKUP for query in keychain):
            raise AssertionError(f"unexpected keychain queries: {keychain}")
        value["queries"] = [query for query in value["queries"] if query["name"] != "security"]
    return value


def replay_case(rust: Path, row: dict, provider: Path, api: dict) -> dict:
    actual = state_key.observe(rust, row["input"], provider, api, extra_env=REPLAY_ENV)
    return {"input": row["input"], "rust": actual,
            "matches": comparable(actual) == comparable(row["go"])}


def run_store_probe(probe: Path, api: dict) -> dict:
    with tempfile.TemporaryDirectory(prefix="bk-store-", dir=registry.process.private_temporary_parent()) as directory:
        result = subprocess.run([str(probe), str(Path(directory) / "new-store")], capture_output=True, timeout=15)
    observed = {"exit": result.returncode, "stdout": result.stdout.decode(), "stderr": result.stderr.decode(),
                "binary_sha256": registry.process.digest(probe)}
    assert result.returncode == 0, observed["stderr"]
    observed["observations"] = json.loads(observed["stdout"])
    observed["matches"] = observed["observations"] == api["observations"] and STORE_WARNING in observed["stderr"]
    return observed


def main() -> int:
    parser = argparse.ArgumentParser(description="Replay the sealed Go state-key recording against Rust.")
    parser.add_argument("--rust", type=Path, required=True, help="Rust symbrowse binary")
    parser.add_argument("--rust-store-probe", type=Path, required=True, help="state_key_store_probe example")
    parser.add_argument("--out", type=Path, required=True, help="receipt path")
    parser.add_argument("--recording-dir", type=Path, default=RECORDING_DIR)
    args = parser.parse_args()
    if os.name != "posix":
        raise SystemExit("state-key replay supports macOS and Linux only (#990)")
    recording = verify_seal(args.recording_dir.resolve())
    check_inputs(recording)
    api = recording["oracle_api"]
    rust = args.rust.resolve()
    rows, controls = [], []
    with tempfile.TemporaryDirectory(prefix="bk-tools-", dir=registry.process.private_temporary_parent()) as directory:
        provider = key_test_environment.build_provider(Path(directory))
        control = fixture_programs.install("owner-control", Path(directory) / "key-control")
        for row in recording["cases"]:
            result = replay_case(rust, row, provider, api)
            rows.append(result)
            print(row["input"]["name"], "matches=", result["matches"], flush=True)
        for mode, name in CONTROLS:
            row = next(row for row in recording["cases"] if row["input"]["name"] == name)
            actual = state_key.observe(rust, row["input"], provider, api, (control, mode), REPLAY_ENV)
            rejected = comparable(actual) != comparable(row["go"])
            controls.append({"name": mode, "input": row["input"], "rejected": rejected,
                             "wrapper_binary_sha256": registry.process.digest(control)})
            print("control", mode, "rejected=", rejected, flush=True)
    store = run_store_probe(args.rust_store_probe.resolve(), api)
    report = {"recording": str((args.recording_dir / RECORDING).relative_to(state_key.ROOT)
                               if args.recording_dir.resolve().is_relative_to(state_key.ROOT) else args.recording_dir),
              "recording_go_ref": recording["go_ref"], "recording_platform": recording["platform"],
              "candidate_head": subprocess.check_output(["git", "-C", str(state_key.ROOT), "rev-parse", "HEAD"], text=True).strip(),
              "platform": platform.platform(), "rust_sha256": registry.process.digest(rust),
              "cases": rows, "startup_pairs": len(rows), "negative_controls": controls, "rust_store_probe": store,
              "matches": all(row["matches"] for row in rows) and all(c["rejected"] for c in controls) and store["matches"]}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"state-key replay: {sum(r['matches'] for r in rows)}/{len(rows)} cases match, "
          f"{sum(c['rejected'] for c in controls)}/{len(controls)} controls rejected, "
          f"store probe {'matches' if store['matches'] else 'differs'}")
    return 0 if report["matches"] and rows else 1


if __name__ == "__main__":
    raise SystemExit(main())
