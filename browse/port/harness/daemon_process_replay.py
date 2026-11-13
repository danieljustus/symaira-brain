#!/usr/bin/env python3
"""Go-free daemon process gate: replay sealed Go process recordings against Rust (#772).

`daemon_process.py` drove a pinned Go binary that was removed in #1003. Its
accepted runs are recorded in `migration/evidence/browse-daemon-772`: each case
starts one daemon in a private root, sends the seven registry frames and keeps
Go's exit, stdout, stderr and responses next to Rust's. This replay keeps the
Go side fixed to those recordings and observes only Rust with the same
`daemon_process.observe`, `normalized` and rejecting controls.

Sealing: every `*process-report*.json` recording is pinned by sha256 below. A
changed or missing recording fails before any case runs, and so does any
`*process-report*.json` that is not pinned.

Recordings come from Linux. On macOS two kinds of cases do not apply and are
counted as platform-unavailable, never as parity: a raw-byte origin directory
cannot be created (EILSEQ, proven per run by `raw_path_admission`), and
`XDG_CACHE_HOME` does not select the cache root.
"""
from __future__ import annotations

import argparse
import copy
import json
import os
from pathlib import Path
import platform
import subprocess

import daemon_process as process
import daemon_registry_replay as layout
import daemon_state_key as state_key
from raw_path_admission import probe, unavailable_is_proven

RECORDING_DIR = state_key.ROOT / "migration/evidence/browse-daemon-772"
# Re-pin only with a reviewed re-record.
PINNED = {
    "linux-process-report.json": "17d680e6ccacdecfb521a3ce48ac2092a41f4dcb65b193f0cfdfad3eb8cb3527",
    "original-linux-process-report.json": "57dae74d6a033b4969316cdcc912e66c91d06ed66d60bf6f1e39267a7a1d006d",
    "linux-mcp-process-report.json": "fe964c183de16fe1668fad341f440f7a3dacf68a0f5aaa1cff8b2c528dcbe9e3",
    "original-linux-mcp-process-report.json": "5b1ecb63bed22c16a65b148f41f918a9aaab1568261a0aa4608b8d25a5ff100e",
}
REPLAYED = ("linux-process-report.json", "original-linux-process-report.json")
REQUESTS_PER_CASE = len(process.COMMANDS)


class SealError(AssertionError):
    """A recording is not the sealed one."""


def verify_seal(directory: Path, pinned: dict[str, str] = PINNED) -> dict[str, dict]:
    failures = []
    present = {path.name for path in directory.glob("*process-report*.json")}
    failures += [f"unregistered recording file: {name}" for name in sorted(present - pinned.keys())]
    failures += [f"missing recording file: {name}" for name in sorted(pinned.keys() - present)]
    failures += [f"changed recording file: {name}" for name in sorted(present & pinned.keys())
                 if process.digest(directory / name) != pinned[name]]
    failures += [f"symlinked recording file: {name}" for name in sorted(present & pinned.keys())
                 if (directory / name).is_symlink()]
    if failures:
        raise SealError("\n".join(failures))
    return {name: json.loads((directory / name).read_text()) for name in REPLAYED}


def check_recording(name: str, report: dict) -> None:
    cases = report["cases"]
    assert report["matches"] and all(case["matches"] for case in cases), f"{name} was not a passing gate"
    assert report["supplemental_go_ref"] == process.GO_REF, f"{name} is from a different Go ref"
    assert report["total"] == len(cases) * REQUESTS_PER_CASE, f"{name} request count drifted"
    for case in cases:
        requests = [record["request"] for record in case["go"]["observations"]]
        assert requests == [{"cmd": command, "session": case["session"]} for command in process.COMMANDS]
        assert case["go"]["exit"] == 0 and case["go"]["stdout_hex"] == ""


def replay_case(rust: Path, case: dict) -> dict:
    raw = None if case.get("raw_origin_hex") is None else bytes.fromhex(case["raw_origin_hex"])
    row = {"session": case["session"], "settings": case["settings"], "raw_origin_hex": case.get("raw_origin_hex"),
           "git_origin": case.get("git_origin", False)}
    if platform.system() == "Darwin" and "XDG_CACHE_HOME" in case["settings"]:
        # Go's os.UserCacheDir ignores XDG_CACHE_HOME on darwin ($HOME/Library/Caches),
        # so the recorded Linux fallback to $TMPDIR does not describe macOS.
        return dict(row, child_executed=False, parity=False, matches=None,
                    reason="XDG_CACHE_HOME selects the cache root on Linux only")
    if raw is not None:
        row["kernel_probe"] = {}
        probe(raw, process.private_temporary_parent(), row["kernel_probe"])
        if not row["kernel_probe"]["admitted"]:
            if not unavailable_is_proven(row["kernel_probe"], raw):
                raise AssertionError("raw filename exclusion lacks exact owned Darwin EILSEQ92 proof")
            return dict(row, child_executed=False, parity=False, matches=None,
                        reason="the kernel rejects the raw filename (EILSEQ 92)")
    observed = layout.project_platform_paths(
        process.observe(rust, case["settings"], case["session"], raw, row["git_origin"]))
    pair = {"session": case["session"], "settings": case["settings"], "go": case["go"], "rust": observed}
    return dict(row, rust=observed, matches=process.compare(pair), pair=pair)


def main() -> int:
    parser = argparse.ArgumentParser(description="Replay the sealed Go daemon process recordings against Rust.")
    parser.add_argument("--rust", type=Path, required=True, help="Rust symbrowse binary")
    parser.add_argument("--out", type=Path, required=True, help="receipt path")
    parser.add_argument("--recording-dir", type=Path, default=RECORDING_DIR)
    args = parser.parse_args()
    if os.name != "posix":
        raise SystemExit("process replay supports macOS and Linux only (#990)")
    recordings = verify_seal(args.recording_dir.resolve())
    rust = args.rust.resolve()
    report = {"recordings": {}, "candidate_head": subprocess.check_output(
        ["git", "-C", str(state_key.ROOT), "rev-parse", "HEAD"], text=True).strip(),
        "platform": platform.platform(), "rust_sha256": process.digest(rust)}
    ok = True
    for name, recording in recordings.items():
        check_recording(name, recording)
        rows = [replay_case(rust, case) for case in recording["cases"]]
        replayed = [row for row in rows if row["matches"] is not None]
        unavailable = [row for row in rows if row["matches"] is None]
        pairs = [row["pair"] for row in replayed]
        negative = process.controls(pairs) if pairs else []
        matched = all(row["matches"] for row in replayed) and bool(replayed)
        ok = ok and matched and len(negative) == 3
        report["recordings"][name] = {
            "sha256": PINNED[name], "recorded_cases": len(recording["cases"]), "replayed_cases": len(replayed),
            "platform_unavailable_cases": [{k: row[k] for k in ("session", "settings", "raw_origin_hex", "reason")}
                                           for row in unavailable],
            "requests_replayed": len(replayed) * REQUESTS_PER_CASE, "matches": matched,
            "negative_controls": negative,
            "cases": [{k: v for k, v in row.items() if k != "pair"} for row in rows]}
        print(f"{name}: {len(replayed)}/{len(recording['cases'])} cases replayed "
              f"({len(replayed) * REQUESTS_PER_CASE} daemon requests), matches={matched}, "
              f"{len(unavailable)} platform-unavailable, {len(negative)} controls rejected", flush=True)
        for row in replayed:
            if not row["matches"]:
                print("difference", row["session"], row["settings"])
    report["matches"] = ok
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
