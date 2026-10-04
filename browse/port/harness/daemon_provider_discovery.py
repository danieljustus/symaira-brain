#!/usr/bin/env python3
"""Actual Go/native default startup discovery; stop every owned daemon first."""
from __future__ import annotations
import argparse
import base64
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import daemon_state_key as key
from daemon_state_key_ownership import Lease

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def build(directory: Path, tool: str, *, control: bool = False) -> Path:
    source = directory / "source"; source.mkdir()
    shutil.copyfile(HERE / ("discovery_owner_control.go.in" if control else "discovery_provider.go.in"), source / "main.go")
    if not control:
        companion = "windows" if os.name == "nt" else "unix"
        shutil.copyfile(HERE / f"discovery_provider_{companion}.go.in", source / "vectors.go")
    binary = directory / ("fixture.exe" if os.name == "nt" else "fixture")
    env = dict(os.environ, CGO_ENABLED="0", GO111MODULE="off", GOTOOLCHAIN="local")
    subprocess.run([tool, "build", "-o", str(binary), str(source / "main.go"), *([] if control else [str(source / "vectors.go")])], env=env, capture_output=True, timeout=120, check=True)
    return binary


def stop_owned(endpoint: Path, session: str, actual_binary: Path) -> dict:
    status = key.registry.harness.request(endpoint, {"cmd": "daemon.status", "session": session})
    pid = status["data"]["pid"]
    lease = Lease(pid, actual_binary)
    try:
        assert lease.alive(), "autostart owner exited before cleanup"
        stopped = key.registry.harness.request(endpoint, {"cmd": "daemon.stop", "session": session})
        deadline = time.monotonic() + 10
        while lease.alive() and time.monotonic() < deadline: time.sleep(.02)
        assert not lease.alive(), "owned daemon did not stop before fixture teardown"
        return {"pid": pid, "response": stopped, "gone_before_teardown": True}
    finally:
        # This only holds the verified owned process identity; never kill by name.
        lease.cleanup()


def install(provider: Path, target: Path, *, permission: int = 0o700):
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(provider, target); target.chmod(permission)


def observe(binary: Path, actual_binary: Path, provider: Path, case: str, *, metadata: bool, control: bool = False) -> dict:
    with tempfile.TemporaryDirectory(prefix="bd-", dir=key.registry.process.private_temporary_parent()) as folder:
        root = Path(folder); env = key.registry.environment(root)
        suffix = ".exe" if os.name == "nt" else ""
        local = root / ("symvault" + suffix); first = root / "bin" / local.name; second = root / "later" / local.name
        for target in (local, first, second): install(provider, target)
        # The original ten Linux observations retain their exact PATH values.
        # Darwin additionally owns a legitimate exit44 security, never treating
        # an undiscoverable keychain command as a successful absence fixture.
        keychain = root / "keychain"
        if os.sys.platform == "darwin": install(provider, keychain / "security")
        separator = os.pathsep
        values = {"empty": "", "dot": ".", "dot-bin": "./bin", "relative-bin": "bin", "absolute": str(first.parent),
                  "relative-first": "bin" + separator + str(second.parent), "absolute-first": str(second.parent) + separator + "bin",
                  "missing-first": "absent" + separator + str(second.parent), "nonexec-first": "bin" + separator + str(second.parent),
                  "directory-first": "bin" + separator + str(second.parent), "empty-entry-first": separator + str(second.parent),
                  "opt-in-dot": ".", "opt-in-relative": "bin", "opt-in-empty": "", "last-opt-in": ".", "last-refusal": "."}
        path = values.get(case, str(first.parent))
        if case == "nonexec-first": first.chmod(0o600)
        if case == "directory-first": first.unlink(); first.mkdir()
        if case in ("opt-in-dot", "opt-in-relative", "opt-in-empty", "last-opt-in"): env["GODEBUG"] = "execerrdot=0"
        if case == "last-opt-in": env["GODEBUG"] = "execerrdot=1,execerrdot=0"
        if case == "last-refusal": env["GODEBUG"] = "execerrdot=0,execerrdot=1"
        if os.name == "nt":
            env["NoDefaultCurrentDirectoryInExePath"] = "1"
            if case.startswith("implicit-"): env.pop("NoDefaultCurrentDirectoryInExePath")
            if case == "implicit-empty": path = ""
            if case == "implicit-absolute-other": path = str(first.parent)
            if case == "implicit-same": path = str(root)
            if case == "implicit-hardlink": first.unlink(); os.link(local, first); path = str(first.parent)
            if case == "implicit-symlink": first.unlink(); first.symlink_to(local); path = str(first.parent)
            if case == "pathext-order":
                install(provider, first.with_suffix(".com")); env["PATHEXT"] = ".COM;.EXE"; path = str(first.parent)
            if case == "pathext-no-dot": env["PATHEXT"] = "EXE"; path = str(first.parent)
            if case == "pathext-empty-list":
                install(provider, first.with_suffix("")); env["PATHEXT"] = ";;"; path = str(first.parent)
            if case == "raw-wide-path":
                wide = root / "raw-\ud800" / local.name; install(provider, wide); path = str(wide.parent)
            if case == "raw-wide-pathext":
                install(provider, first.with_suffix(".e\ufffd\ufffd\ufffd")); env["PATHEXT"] = ".E\ud800"; path = str(first.parent)
        if os.sys.platform == "darwin": path += separator + str(keychain)
        ledger = root / "queries.jsonl"
        env.update(PATH=path, SYMBROWSE_KEY_PROBE_MODE="ab", SYMBROWSE_KEYCHAIN_PROBE_MODE="44", SYMBROWSE_KEY_PROBE_LEDGER=str(ledger), SYMBROWSE_ENCRYPTION_KEY=key.KEY)
        if control: env.update(DISCOVERY_OWNED_ROOT=str(root), DISCOVERY_ACTUAL_RUST=str(actual_binary))
        session = f"d{os.getpid():x}{int(time.monotonic_ns()):x}"[-24:]
        endpoint = key.registry.harness.daemon_socket_path(Path(env["XDG_RUNTIME_DIR"]), session)
        command = [str(binary), "state", "list", "--session", session, "--json"]
        try:
            result = subprocess.run(command, env=env, cwd=root, capture_output=True, timeout=12)
        finally:
            cleanup = stop_owned(endpoint, session, actual_binary)
        # All prepared cases resolve or deliberately skip a public AB provider,
        # with a legitimate owned Darwin keychain. A failed startup is a gate
        # failure, not permission to silently leave its owner behind.
        assert result.returncode == 0 and not result.stderr, (case, result.returncode, result.stderr)
        queries = [json.loads(line) for line in ledger.read_text().splitlines()] if ledger.exists() else []
        vault = [q for q in queries if q.get("name", "symvault") != "security"]
        vectors = []
        if metadata:
            vault = []
            for query in queries:
                executable = Path(os.fsdecode(base64.b64decode(query["executable_base64"])))
                assert executable.is_relative_to(root) and executable.is_file()
                assert key.registry.process.digest(executable) == key.registry.process.digest(provider)
                assert isinstance(query["pid"], int) and query["pid"] > 0
                if executable.name == "security": continue
                assert query["arguments"] == ["get", "symbrowse/encryption-key"]
                vault.append(query); vectors.append({**query, "owned_executable_relative": str(executable.relative_to(root))})
        else:
            for query in vault:
                assert query["arguments"] == ["get", "symbrowse/encryption-key"] and query["pid"] > 0
        return {"case": case, "root": str(root), "actual_path": path, "arguments": command[1:], "exit": result.returncode,
                "stdout_base64": base64.b64encode(result.stdout).decode(), "stderr_base64": base64.b64encode(result.stderr).decode(),
                "queries": queries, "vault_query_count": len(vault), "vectors": vectors, "owner_paths": [v["owned_executable_relative"] for v in vectors], "cleanup": cleanup,
                "provider_sha256": key.registry.process.digest(provider), "binary_sha256": key.registry.process.digest(binary),
                "discovery_environment_base64": {name: base64.b64encode(os.fsencode(env[name])).decode() for name in ("PATH", "PATHEXT", "GODEBUG", "NoDefaultCurrentDirectoryInExePath") if name in env},
                "discovery_environment_utf16_base64": {name: base64.b64encode(env[name].encode("utf-16le", "surrogatepass")).decode() for name in ("PATH", "PATHEXT", "GODEBUG", "NoDefaultCurrentDirectoryInExePath") if name in env} if os.name == "nt" else None}


def pairs(go: Path, rust: Path, provider: Path, cases: list[str], *, metadata: bool) -> list:
    rows = []
    for case in cases:
        left = observe(go, go, provider, case, metadata=metadata)
        right = observe(rust, rust, provider, case, metadata=metadata)
        fields = ["exit", "stdout_base64", "stderr_base64", "vault_query_count"] + (["owner_paths"] if metadata else [])
        assert all(left[field] == right[field] for field in fields), (case, left, right)
        if metadata and os.name != "nt":
            assert [v["argv0_base64"] for v in left["vectors"]] == [v["argv0_base64"] for v in right["vectors"]]
        rows.append({"case": case, "go": left, "rust": right, "matches": True,
                     "argv0_equal": [v["argv0_base64"] for v in left["vectors"]] == [v["argv0_base64"] for v in right["vectors"]],
                     "argv0_and_raw_command_line": "raw vectors retained separately for native ownership/argv review; no blanket Windows argv0 equivalence claim"})
    return rows


def script_vectors(probe: Path, owner: Path, tool: str, tools: Path) -> list:
    if os.name != "nt": return []
    source = tools / "sdk-oracle.go"; shutil.copyfile(HERE / "discovery_sdk_oracle.go.in", source)
    oracle = tools / "sdk-oracle.exe"
    subprocess.run([tool, "build", "-o", str(oracle), str(source)], env=dict(os.environ, CGO_ENABLED="0", GO111MODULE="off", GOTOOLCHAIN="local"), capture_output=True, timeout=120, check=True)
    rows = []
    for extension in ("bat", "cmd"):
        with tempfile.TemporaryDirectory(prefix="bd-script-", dir=key.registry.process.private_temporary_parent()) as folder:
            root = Path(folder); env = key.registry.environment(root); directory = root / "bin"; directory.mkdir(); marker = root / "shell-was-executed"
            script = directory / ("symvault." + extension)
            script.write_bytes(b'@echo off\r\necho forbidden-shell-owner > "%DISCOVERY_MARKER%"\r\n')
            env.update(PATH=str(directory), PATHEXT="." + extension.upper(), NoDefaultCurrentDirectoryInExePath="1", DISCOVERY_MARKER=str(marker), SYMBROWSE_ENCRYPTION_KEY=key.KEY)
            actual_go = subprocess.run([str(oracle)], env=env, cwd=root, capture_output=True, timeout=5)
            actual_rust = subprocess.run([str(probe), "symvault", str(owner)], env=env, cwd=root, capture_output=True, timeout=5)
            assert actual_go.returncode == actual_rust.returncode == 0 and not actual_go.stderr and not actual_rust.stderr
            go_value, rust_value = json.loads(actual_go.stdout), json.loads(actual_rust.stdout)
            assert not go_value["lookup_error"] and go_value["invoke_error"]
            prefix = 'symvault entry "symbrowse/encryption-key": '
            assert rust_value["error"] == prefix + go_value["invoke_error"] and not rust_value["configured"]
            assert not marker.exists(), "native changed Go failed CreateProcess into shell execution"
            rows.append({"extension": extension, "arguments": ["symvault", str(owner)], "go_stdout_base64": base64.b64encode(actual_go.stdout).decode(), "rust_stdout_base64": base64.b64encode(actual_rust.stdout).decode(), "go": go_value, "rust": rust_value, "shell_executed": False, "script_sha256": key.registry.process.digest(script), "oracle_binary_sha256": key.registry.process.digest(oracle), "matches": True})
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    for name in ("go", "rust", "go-source", "out"): parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--go-tool", default="go")
    parser.add_argument("--retained-provider", type=Path)
    parser.add_argument("--rust-source-probe", type=Path, required=True)
    args = parser.parse_args(); go, rust = args.go.resolve(), args.rust.resolve()
    assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.go_source, text=True).strip() == key.registry.process.GO_REF
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=args.go_source)
    original = ["empty", "dot", "dot-bin", "relative-bin", "absolute"]
    cases = original + ["relative-first", "absolute-first", "missing-first", "directory-first", "empty-entry-first", "opt-in-dot", "opt-in-relative", "opt-in-empty", "last-opt-in", "last-refusal"]
    if os.name == "nt": cases += ["implicit-empty", "implicit-absolute-other", "implicit-same", "implicit-hardlink", "implicit-symlink", "pathext-order", "pathext-no-dot", "pathext-empty-list", "raw-wide-path", "raw-wide-pathext"]
    else: cases.append("nonexec-first")
    with tempfile.TemporaryDirectory(prefix="bd-tools-", dir=key.registry.process.private_temporary_parent()) as folder:
        tools = Path(folder); provider_dir = tools / "provider"; provider_dir.mkdir(); provider = build(provider_dir, args.go_tool)
        rows = pairs(go, rust, provider, cases, metadata=True)
        scripts = script_vectors(args.rust_source_probe.resolve(), rust, args.go_tool, tools)
        original_rows = pairs(go, rust, args.retained_provider.resolve(), original, metadata=False) if args.retained_provider else []
        control_dir = tools / "control"; control_dir.mkdir(); control = build(control_dir, args.go_tool, control=True)
        mutated = observe(control, rust, provider, "empty-entry-first", metadata=True, control=True)
        ordinary = next(row for row in rows if row["case"] == "empty-entry-first")
        assert mutated["vault_query_count"] != ordinary["go"]["vault_query_count"], "actual false-owner mutant escaped"
        report = {"candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                  "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                  "go_ref": key.registry.process.GO_REF, "go_version": subprocess.check_output([args.go_tool, "version"], text=True).strip(),
                  "go_binary_sha256": key.registry.process.digest(go), "rust_binary_sha256": key.registry.process.digest(rust),
                  "cases": rows, "original_retained_provider_pairs": original_rows, "windows_script_sdk_vectors": scripts, "source_probe_sha256": key.registry.process.digest(args.rust_source_probe), "matches": True,
                  "negative_control": {"observed": mutated, "binary_sha256": key.registry.process.digest(control), "rejected": True},
                  "scope": "actual native default startup CLI owner/query/bytes; every autostart daemon identity stopped and waited before teardown; Linux original10 retained when provided. Windows argv0/rawGetCommandLine remain explicit observations requiring independent review."}
        files = subprocess.check_output(["git", "ls-files", "browse/crates", "browse/port/harness"], cwd=ROOT, text=True).splitlines()
        report["candidate_source_sha256"] = {p: key.registry.process.digest(ROOT / p) for p in files}
        args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"{len(rows)} actual provider-discovery pairs + {len(original_rows)} original retained pairs; actual false-owner child rejected; all owners gone before teardown")
    return 0


if __name__ == "__main__": raise SystemExit(main())
