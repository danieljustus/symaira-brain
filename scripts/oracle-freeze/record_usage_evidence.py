#!/usr/bin/env python3
"""Historical recorder: freeze the fresh-Go usage evidence gates into one fixture each.

REQUIRES a Go toolchain; NOT part of any gate (it is deleted together with the Go sources).
The legacy gates (`scripts/usage-*-oracle/run.sh`) built the pinned Go revision, ran a supplemental
Go test that wrote evidence JSON plus an on-disk credential tree, and then ran an `#[ignore]`d Rust
test against both. This recorder performs only the Go half and seals the result, so the Rust test
can replay it without Go:

    record_usage_evidence.py <gate> <go-source-dir> <out.json> <scratch-dir> [--go "dev-external go"]

* ``<go-source-dir>`` is an extracted copy of the pinned revision (``git archive dcddcef0... | tar -x``).
* Scratch roots are replaced by ``<ROOT>``/``<BIN>``/``<BASEROOT>`` tokens, the recorded filesystem
  trees are embedded (path, mode, bytes, symlink targets) and restored by the Rust helper
  ``tests/support/frozen_go_evidence.rs`` below a fresh temp root.
"""
from __future__ import annotations

import json
import os
import platform
import shlex
import shutil
import stat
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
ORACLE_COMMIT = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"

GATES = {
    "credential": {
        "tests": [("scripts/usage-credential-oracle/provider_test.go.txt", "oracle_768_test.go")],
        "inputs": {},
        "runs": [("^TestUsageCredentialOracle768$", {"USAGE_768_ORACLE": "out:records", "USAGE_768_ROOT": "root:ROOT", "USAGE_768_BIN": "root:BIN"})],
        "fake_bin": "BIN",
    },
    "hermes": {
        "tests": [("scripts/usage-hermes-oracle/provider_test.go.txt", "oracle_hermes_768_test.go")],
        "inputs": {"input": ["scripts/usage-hermes-oracle/cases.py"]},
        "runs": [("^TestUsageHermesOracle768$", {"USAGE_HERMES_ORACLE": "out:records", "USAGE_HERMES_INPUT": "in:input", "USAGE_HERMES_ROOT": "root:ROOT"})],
    },
    "files": {
        "tests": [("scripts/usage-provider-files-oracle/provider_test.go.txt", "oracle_provider_files_768_test.go")],
        "inputs": {"input": ["scripts/usage-provider-files-oracle/cases.py"]},
        "runs": [("^TestUsageProviderFiles768$", {"USAGE_FILES_ORACLE": "out:records", "USAGE_FILES_INPUT": "in:input", "USAGE_FILES_ROOT": "root:ROOT"})],
    },
    "copilot-kimi": {
        "tests": [
            ("scripts/usage-copilot-kimi-oracle/provider_test.go.txt", "oracle_copilot_kimi_768_test.go"),
            ("scripts/usage-local-files-baseline/provider_test.go.txt", "oracle_local_files_baseline_768_test.go"),
            ("scripts/usage-copilot-kimi-oracle/owner_test.go.txt", "oracle_credential_owner_768_test.go"),
        ],
        "inputs": {"input": ["scripts/usage-copilot-kimi-oracle/cases.py"], "baseline_input": ["scripts/usage-local-files-baseline/cases.py"]},
        "runs": [
            ("^TestUsageCopilotKimi768$", {"USAGE_COPILOT_KIMI_ORACLE": "out:records", "USAGE_COPILOT_KIMI_INPUT": "in:input", "USAGE_COPILOT_KIMI_ROOT": "root:ROOT"}),
            ("^TestUsageLocalFilesBaseline768$", {"USAGE_LOCAL_INPUT": "in:baseline_input", "USAGE_LOCAL_OUTPUT": "out:baseline", "USAGE_LOCAL_ROOT": "root:BASEROOT"}),
            ("^TestUsageCredential(Owner|Path)768$", {"USAGE_COPILOT_KIMI_OWNER": "out:owner", "USAGE_COPILOT_KIMI_PATH": "out:path", "USAGE_COPILOT_KIMI_ROOT": "root:ROOT"}),
        ],
    },
}


def snapshot(root: Path) -> list[dict]:
    entries: list[dict] = []
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root).as_posix()
        info = path.lstat()
        mode = stat.S_IMODE(info.st_mode)
        if path.is_symlink():
            entries.append({"path": rel, "kind": "symlink", "target": os.readlink(path)})
        elif path.is_dir():
            entries.append({"path": rel, "kind": "dir", "mode": mode})
        elif path.is_file():
            entries.append({"path": rel, "kind": "file", "mode": mode, "hex": path.read_bytes().hex()})
        else:
            raise SystemExit(f"unsupported filesystem object recorded: {path}")
    return entries


def main() -> int:
    gate_name, source, out, scratch_arg = sys.argv[1:5]
    go = shlex.split(sys.argv[sys.argv.index("--go") + 1]) if "--go" in sys.argv else ["go"]
    gate = GATES[gate_name]
    source_dir, scratch = Path(source), Path(scratch_arg)
    shutil.rmtree(scratch, ignore_errors=True)
    scratch.mkdir(parents=True)
    toolchain = os.environ.get("RECORD_GOTOOLCHAIN", "go1.26.7")  # the pin of the legacy gates
    env_base = {**os.environ, "GOTOOLCHAIN": toolchain, "GOPROXY": "off", "CGO_ENABLED": "0", "PYTHONUTF8": "1"}
    paths: dict[str, Path] = {}
    for name, command in gate["inputs"].items():
        target = scratch / f"{name}.json"
        subprocess.run([sys.executable, str(REPO / command[0]), str(target)], check=True, cwd=REPO, env=env_base)
        paths[name] = target
    for token in ("ROOT", "BIN", "BASEROOT"):
        (scratch / token.lower()).mkdir()
    if gate.get("fake_bin"):
        bin_dir = scratch / gate["fake_bin"].lower()
        subprocess.run(["rustc", "--edition=2024", str(REPO / "rust/symbrain-usage/tests/support/fake_symvault.rs"), "-o", str(bin_dir / "symvault")], check=True)
        shutil.copyfile(bin_dir / "symvault", bin_dir / "security")
        os.chmod(bin_dir / "security", 0o755)
    for src, dst in gate["tests"]:
        shutil.copyfile(REPO / src, source_dir / "internal/usage" / dst)
    tokens = {"<ROOT>": scratch / "root", "<BIN>": scratch / "bin", "<BASEROOT>": scratch / "baseroot"}
    outputs: dict[str, Path] = {}
    for regex, mapping in gate["runs"]:
        env = dict(env_base)
        for variable, spec in mapping.items():
            kind, _, name = spec.partition(":")
            if kind == "out":
                outputs[name] = scratch / f"{name}.json"
                env[variable] = str(outputs[name])
            elif kind == "in":
                env[variable] = str(paths[name])
            else:
                env[variable] = str(tokens[f"<{name}>"])
        subprocess.run([*go, "-C", str(source_dir), "test", "./internal/usage", "-run", regex, "-count=1"], check=True, env=env)
    documents = {name: json.loads(path.read_text(encoding="utf-8")) for name, path in outputs.items()}
    documents.update({name: json.loads(paths[name].read_text(encoding="utf-8")) for name in gate["inputs"]})
    trees = {token: snapshot(path) for token, path in tokens.items() if token != "<BIN>" and any(path.iterdir())}
    text = json.dumps({"documents": documents, "trees": trees}, sort_keys=True)
    # longest roots first so "<BASEROOT>" is not shadowed by a shorter prefix
    for token, path in sorted(tokens.items(), key=lambda item: -len(str(item[1]))):
        text = text.replace(json.dumps(str(path))[1:-1], token)
    body = json.loads(text)
    version = subprocess.run([*go, "version"], capture_output=True, text=True, check=True, env=env_base).stdout.strip()
    document = {
        "schema_version": 1,
        "provenance": {
            "recorder": "scripts/oracle-freeze/record_usage_evidence.py " + gate_name,
            "gate": gate_name,
            "oracle_commit": ORACLE_COMMIT,
            "go_version": version,
            "gotoolchain": toolchain,
            "go_test_runs": [{"run": regex, "env": sorted(mapping)} for regex, mapping in gate["runs"]],
            "platform": f"{platform.system().lower()}/{platform.machine()}",
        },
        "tokens": sorted(tokens),
        **body,
    }
    Path(out).write_text(json.dumps(document, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    print(f"recorded {gate_name}: documents={sorted(documents)} trees={ {k: len(v) for k, v in trees.items()} }")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
