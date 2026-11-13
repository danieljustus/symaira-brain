#!/usr/bin/env python3
"""Go-free registry gate: replay the sealed Go registry recording against Rust (#772).

The historical gate (`daemon_registry.py`) drove a pinned Go binary and Go
probe programs that were removed in #1003. Its last accepted run recorded every
Go observation next to the Rust one (DMN-007 registry frames, DMN-008 concurrent
autostart clients, plus the literal CLI edge cases). This replay keeps the Go
side fixed to that sealed recording and observes only the Rust candidate with
the same harness code, the same normalization and the same rejecting controls.

It also replays the 12 sealed Go observations of the root-prefix
`--session <id> <group> <verb>` spelling against the current Rust CLI.

Sealing: `retention.json` in the recording directory lists the sha256 of every
retained file, and its own sha256 is pinned in `daemon_state_key_replay`. A
changed, missing or unregistered recording file fails before any case runs.

Local macOS runs need a short `SYMAIRA_EXTERNAL_RUNTIME_ROOT`/TMPDIR: the
daemon socket path must stay below the Unix sun_path limit.
"""
from __future__ import annotations

import argparse
import copy
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tempfile

import daemon_registry as registry
import daemon_state_key as state_key
import daemon_state_key_replay as sealed
from registry_compare import compare, controls, normalize

RECORDING_DIR = sealed.RECORDING_DIR
RECORDING = "symaira-state-key-final-registry.json"
PREFIX_RECORDING = "symaira-state-key-4cd-root-prefix-observations.json"
# Release builds back up pre-Rust state once on first run (symaira-cutover);
# the historical Go recording predates that notice, so the replay opts out.
REPLAY_ENV = sealed.REPLAY_ENV
PREFIX_SESSION = "owned-prefix"
PREFIX_ROOT = re.compile(r"/tmp/bk-prefix-[A-Za-z0-9_]+")
PREFIX_CONTROLS = ("exit", "stdout", "stderr")
DARWIN_RUN = "<owned-root>/home/Library/Caches/symbrowse/run/"
LINUX_RUN = "<owned-root>/run/symbrowse/"


def load(directory: Path) -> tuple[dict, dict]:
    """Seal-verify the directory and return (registry, root-prefix) recordings."""
    recording = sealed.verify_seal(directory, recording=RECORDING)
    prefix = json.loads((directory / PREFIX_RECORDING).read_text())
    assert PREFIX_RECORDING in json.loads((directory / sealed.RETENTION).read_text())["files_sha256"], \
        f"{PREFIX_RECORDING} is not part of the seal"
    return recording, prefix


def check_registry_recording(recording: dict) -> None:
    """The recording must be a passing run with the counts the contract names."""
    assert recording["matches"] and recording["case"]["matches"], "registry recording was not a passing gate"
    assert recording["go_ref"] == registry.process.GO_REF, "registry recording is from a different Go ref"
    counts = recording["counts_per_binary"]
    go = recording["case"]["go"]
    edges = go["cli_edges"]
    total = sum(len(go[group]) for group in ("missing", "invalid_cli", "inspection", "state_commands", "autostart"))
    total += sum(len(edges[group]) for group in ("invalid", "format_errors", "help"))
    assert counts["cli_observations"] == total == 510, f"recorded CLI observation count drifted: {total}"
    assert len(go["cli_edges"]["invalid"]) == counts["literal_cli_edge_observations"] == 392
    assert len(go["invalid_cli"]) == counts["invalid_session_cli_observations"] == 48
    assert len(go["cli_edges"]["help"]) == counts["implemented_help_observations"] == 6
    assert len(go["cli_edges"]["format_errors"]) == counts["selected_invalid_output_observations"] == 4
    assert len(go["autostart"]) == counts["concurrent_clients"] == 8
    assert len(go["registry"]) == 17
    assert len(recording["oracle_api"]["observations"]["state_fixtures_hex"]) == counts["persisted_go_fixtures"] == 3


def project_platform_paths(observed: dict) -> dict:
    """Map macOS directory layout and realpath spelling onto the recording's.

    The sealed recordings are from Linux, where the runtime and profile
    directories sit under `<root>/run/symbrowse` and `<root>/cache/symbrowse`.
    On macOS the same directories live under `<root>/home/Library/Caches`, and
    the daemon reports the realpath (`/private/tmp/...`) of its working
    directory. Only these three path prefixes are rewritten, in Rust strings;
    every other byte is compared as recorded.
    """
    if platform.system() != "Darwin":
        return observed
    root = observed["root"]
    caches = root + "/home/Library/Caches/symbrowse"
    rewrites = (("/private" + root, root), (caches + "/run/", root + "/run/symbrowse/"),
                (caches + "/sessions/", root + "/cache/symbrowse/sessions/"))

    def walk(item):
        if isinstance(item, dict):
            return {key: walk(value) for key, value in item.items()}
        if isinstance(item, list):
            return [walk(value) for value in item]
        if isinstance(item, str):
            for old, new in rewrites:
                item = item.replace(old, new)
        return item
    return walk(observed)


def replay_registry(rust: Path, recording: dict) -> dict:
    go = recording["case"]["go"]
    fixtures = recording["oracle_api"]["observations"]["state_fixtures_hex"]
    # The recorded session name is reused so every literal session argument and
    # response matches; the private root and daemon PIDs are fresh per run.
    observed = project_platform_paths(registry.observe(rust, go["session"], fixtures))
    case = {"go": go, "rust": observed}
    case["matches"] = compare(case)
    if not case["matches"]:
        left, right = normalize(case["go"], rust=False), normalize(case["rust"], rust=True)
        for key in left:
            if left[key] != right[key]:
                print(f"difference in {key}: recorded Go={left[key]!r}; Rust={right[key]!r}")
    case["revived_owner_counts"] = {"go": 0, "rust": len(observed["revived_owners"])}
    return case


def prefix_comparable(observation: dict, root_pattern: str | None = None) -> dict:
    """Project one root-prefix CLI observation; the private root is replaced."""
    value = copy.deepcopy(observation)
    pattern = re.compile(root_pattern) if root_pattern else PREFIX_ROOT
    for key in ("stdout", "stderr"):
        value[key] = pattern.sub("<owned-root>", value[key])
        if platform.system() == "Darwin":
            # The Linux recording keeps sockets in $XDG_RUNTIME_DIR/symbrowse; on
            # macOS the daemon's runtime directory is the user cache directory.
            value[key] = value[key].replace(DARWIN_RUN, LINUX_RUN)
    return value


def run_prefix(rust: Path, prefix: dict) -> list[dict]:
    rows = []
    with tempfile.TemporaryDirectory(prefix="bk-prefix-", dir=registry.process.private_temporary_parent()) as temporary:
        root = Path(temporary)
        env = registry.environment(root)
        env["SYMBROWSE_NO_AUTOSTART"] = "1"
        env.update(REPLAY_ENV)
        before = sorted(p.relative_to(root).as_posix() for p in root.rglob("*"))
        for case in prefix["cases"]:
            actual = registry.cli(rust, root, env, case["arguments"])
            rows.append({"arguments": case["arguments"], "go": case["go"], "rust": actual})
        assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*")) == before, \
            "root-prefix CLI created daemon/profile/state files"
        escaped = re.escape(str(root))
        for row in rows:
            row["rust_projected"] = prefix_comparable(row["rust"], escaped)
            row["go_projected"] = prefix_comparable(row["go"])
            row["matches"] = row["rust_projected"] == row["go_projected"]
    return rows


def prefix_controls(rows: list[dict]) -> list[dict]:
    """Each altered Rust observation must stop matching the recorded Go one."""
    results = []
    for field in PREFIX_CONTROLS:
        row = copy.deepcopy(rows[0])
        row["rust_projected"][field] = 7 if field == "exit" else row["rust_projected"][field] + " changed"
        results.append({"name": f"prefix-{field}", "rejected": row["rust_projected"] != row["go_projected"]})
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description="Replay the sealed Go registry recording against Rust.")
    parser.add_argument("--rust", type=Path, required=True, help="Rust symbrowse binary")
    parser.add_argument("--out", type=Path, required=True, help="receipt path")
    parser.add_argument("--recording-dir", type=Path, default=RECORDING_DIR)
    args = parser.parse_args()
    if os.name != "posix":
        raise SystemExit("registry replay supports macOS and Linux only (#990)")
    recording, prefix = load(args.recording_dir.resolve())
    check_registry_recording(recording)
    assert len(prefix["cases"]) == 12, "root-prefix recording must hold 12 Go observations"
    assert all(row["go"]["exit"] == 1 for row in prefix["cases"]), "root-prefix Go outputs changed"
    rust = args.rust.resolve()
    case = replay_registry(rust, recording)
    negative = controls(case)
    rows = run_prefix(rust, prefix)
    prefix_negative = prefix_controls(rows)
    report = {"recording": RECORDING, "prefix_recording": PREFIX_RECORDING,
              "recording_go_ref": recording["go_ref"], "recording_platform": recording["platform"],
              "candidate_head": subprocess.check_output(["git", "-C", str(state_key.ROOT), "rev-parse", "HEAD"], text=True).strip(),
              "platform": platform.platform(), "rust_sha256": registry.process.digest(rust),
              "counts_per_binary": recording["counts_per_binary"],
              "registry_matches": case["matches"], "revived_owner_counts": case["revived_owner_counts"],
              "negative_controls": negative, "prefix_cases": rows, "prefix_negative_controls": prefix_negative,
              "matches": case["matches"] and all(row["matches"] for row in rows)
              and all(c["rejected"] for c in prefix_negative)}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    counts = recording["counts_per_binary"]
    print(f"registry replay: {counts['cli_observations']} CLI observations + {counts['recorded_raw_frames']} raw frames "
          f"+ {counts['concurrent_clients']} concurrent clients: matches={case['matches']}; "
          f"{len(negative)} controls rejected")
    print(f"root-prefix replay: {sum(row['matches'] for row in rows)}/{len(rows)} cases match; "
          f"{sum(c['rejected'] for c in prefix_negative)}/{len(prefix_negative)} controls rejected")
    for row in rows:
        if not row["matches"]:
            print("prefix difference", row["arguments"], "Go=", row["go_projected"], "Rust=", row["rust_projected"])
    return 0 if report["matches"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
