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
from registry_cli_process import capture
from registry_daemon_lifetime import WindowsJob

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


class RawEvidence:
    """Persist each actual result before parity assertions can fail."""
    def __init__(self, path: Path, binding: dict):
        self.path = path
        self.value = {"binding": binding, "events": [], "complete": False}
        self.flush()

    def flush(self):
        temporary = self.path.with_suffix(self.path.suffix + ".tmp")
        temporary.write_text(json.dumps(self.value, indent=2) + "\n")
        temporary.replace(self.path)

    def append(self, event: dict):
        self.value["events"].append(event)
        self.flush()


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


def owned_executable_relative(executable: Path, root: Path, provider: Path) -> str:
    """Validate canonical ownership while leaving raw observations untouched."""
    owned_root = root.resolve(strict=True)
    owned_executable = executable.resolve(strict=True)
    assert owned_executable.is_relative_to(owned_root) and owned_executable.is_file()
    assert key.registry.process.digest(owned_executable) == key.registry.process.digest(provider)
    return str(owned_executable.relative_to(owned_root))


def observe(binary: Path, actual_binary: Path, provider: Path, case: str, *, evidence: RawEvidence, metadata: bool, control: bool = False, control_mode: str = "cwd") -> dict:
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
                  "opt-in-dot": ".", "opt-in-relative": "bin", "opt-in-empty": "", "last-opt-in": ".", "last-refusal": ".",
                  "missing-dotdot": str(root / "missing" / ".." / "bin"),
                  "symlink-dotdot": str(root / "link" / ".." / "bin"),
                  "relative-missing-dotdot": str(Path("missing") / ".." / "bin"),
                  "suffix-empty": ".", "suffix-quiet-all": ".", "suffix-quiet-none": ".",
                  "suffix-last-opt-in": ".", "suffix-last-refusal": ".", "suffix-invalid": ".",
                  "suffix-quiet-partition-all": "."}
        path = values.get(case, str(first.parent))
        if case == "nonexec-first": first.chmod(0o600)
        if case == "directory-first": first.unlink(); first.mkdir()
        if case == "symlink-dotdot":
            nested = root / "other" / "nested"; nested.mkdir(parents=True)
            install(provider, root / "other" / "bin" / local.name)
            (root / "link").symlink_to(nested, target_is_directory=True)
        if case in ("opt-in-dot", "opt-in-relative", "opt-in-empty", "last-opt-in"): env["GODEBUG"] = "execerrdot=0"
        if case == "last-opt-in": env["GODEBUG"] = "execerrdot=1,execerrdot=0"
        if case == "last-refusal": env["GODEBUG"] = "execerrdot=0,execerrdot=1"
        suffixes = {"suffix-empty": "execerrdot=0#", "suffix-quiet-all": "execerrdot=0#qy",
                    "suffix-quiet-none": "execerrdot=0#qn", "suffix-last-opt-in": "execerrdot=1,execerrdot=0#qy",
                    "suffix-last-refusal": "execerrdot=0#qy,execerrdot=1", "suffix-invalid": "execerrdot=0#not-a-pattern",
                    "suffix-quiet-partition-all": "execerrdot=0#q0+1"}
        impossible_suffixes = {"suffix-impossible-hex": "qxyf", "suffix-impossible-lower": "qxya",
                               "suffix-impossible-upper": "qxyF", "suffix-impossible-digit": "qxy9",
                               "suffix-impossible-inverted": "q!xyf", "suffix-impossible-double-inverted": "q!!xyf",
                               "suffix-impossible-subtract": "q-xyf", "suffix-impossible-prior-all": "qy-xyf",
                               "suffix-impossible-prior-inverted-all": "q!y-xyf", "suffix-impossible-later-all": "qxyf+y",
                               "suffix-impossible-later-partition": "qxyf+0+1", "suffix-impossible-prior-partition": "q0+1-xyf",
                               "suffix-impossible-later-none": "qxyf+y-y", "suffix-invalid-hex": "qxy0"}
        suffixes.update({name: "execerrdot=0#" + pattern for name, pattern in impossible_suffixes.items()})
        if case in impossible_suffixes: path = "."
        if case in suffixes: env["GODEBUG"] = suffixes[case]
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
        if control: env.update(DISCOVERY_OWNED_ROOT=str(root), DISCOVERY_ACTUAL_RUST=str(actual_binary), DISCOVERY_OWNER_CONTROL_MODE=control_mode)
        session = f"d{os.getpid():x}{int(time.monotonic_ns()):x}"[-24:]
        endpoint = key.registry.harness.daemon_socket_path(Path(env["XDG_RUNTIME_DIR"]), session)
        command = [str(binary), "state", "list", "--session", session, "--json"]
        result = None; run_error = cleanup_error = None; cleanup = None
        # Owns the autostarted daemon tree (cwd = root) beyond its PID lease.
        job = WindowsJob() if os.name == "nt" else None
        try:
            # File-backed and bounded: on Windows, subprocess.run's post-timeout
            # communicate() is unbounded while any descendant holds its pipes.
            result = capture(binary, root, env, command[1:], job=job)
        except Exception as error:
            run_error = error
        finally:
            try:
                cleanup = stop_owned(endpoint, session, actual_binary)
            except Exception as error:
                cleanup_error = error
            try:
                if job is not None: job.finish()
            except Exception as error:
                cleanup_error = cleanup_error or error
        # Capture process failures and cleanup failures before reading/parsing
        # provider output or asserting parity. The owned root is still present.
        evidence.append({"phase": "cli.completed", "case": case, "control": control,
                         "binary": str(binary), "binary_sha256": key.registry.process.digest(binary),
                         "provider_sha256": key.registry.process.digest(provider),
                         "root": str(root), "actual_path": path, "arguments": command[1:],
                         "discovery_environment_base64": {name: base64.b64encode(os.fsencode(env[name])).decode() for name in ("PATH", "PATHEXT", "GODEBUG", "NoDefaultCurrentDirectoryInExePath") if name in env},
                         "discovery_environment_utf16_base64": {name: base64.b64encode(env[name].encode("utf-16le", "surrogatepass")).decode() for name in ("PATH", "PATHEXT", "GODEBUG", "NoDefaultCurrentDirectoryInExePath") if name in env} if os.name == "nt" else None,
                         "exit": result.returncode if result is not None else None,
                         "stdout_base64": base64.b64encode(result.stdout if result is not None else getattr(run_error, "stdout", None) or b"").decode(),
                         "stderr_base64": base64.b64encode(result.stderr if result is not None else getattr(run_error, "stderr", None) or b"").decode(),
                         "ledger_base64": base64.b64encode(ledger.read_bytes()).decode() if ledger.exists() else None,
                         "cleanup": cleanup, "run_error": repr(run_error) if run_error else None,
                         "cleanup_error": repr(cleanup_error) if cleanup_error else None})
        if run_error is not None: raise run_error
        if cleanup_error is not None: raise cleanup_error
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
                relative = owned_executable_relative(executable, root, provider)
                assert isinstance(query["pid"], int) and query["pid"] > 0
                if executable.name == "security": continue
                assert query["arguments"] == ["get", "symbrowse/encryption-key"]
                vault.append(query); vectors.append({**query, "owned_executable_relative": relative})
        else:
            for query in vault:
                assert query["arguments"] == ["get", "symbrowse/encryption-key"] and query["pid"] > 0
        return {"case": case, "root": str(root), "actual_path": path, "arguments": command[1:], "exit": result.returncode,
                "stdout_base64": base64.b64encode(result.stdout).decode(), "stderr_base64": base64.b64encode(result.stderr).decode(),
                "queries": queries, "vault_query_count": len(vault), "vectors": vectors, "owner_paths": [v["owned_executable_relative"] for v in vectors], "cleanup": cleanup,
                "provider_sha256": key.registry.process.digest(provider), "binary_sha256": key.registry.process.digest(binary),
                "discovery_environment_base64": {name: base64.b64encode(os.fsencode(env[name])).decode() for name in ("PATH", "PATHEXT", "GODEBUG", "NoDefaultCurrentDirectoryInExePath") if name in env},
                "discovery_environment_utf16_base64": {name: base64.b64encode(env[name].encode("utf-16le", "surrogatepass")).decode() for name in ("PATH", "PATHEXT", "GODEBUG", "NoDefaultCurrentDirectoryInExePath") if name in env} if os.name == "nt" else None}


def pairs(go: Path, rust: Path, provider: Path, cases: list[str], *, evidence: RawEvidence, metadata: bool) -> list:
    rows = []
    for case in cases:
        left = observe(go, go, provider, case, evidence=evidence, metadata=metadata)
        right = observe(rust, rust, provider, case, evidence=evidence, metadata=metadata)
        evidence.append({"phase": "pair.observed", "case": case, "metadata": metadata, "go": left, "rust": right})
        fields = ["exit", "stdout_base64", "stderr_base64", "vault_query_count"] + (["owner_paths"] if metadata else [])
        assert all(left[field] == right[field] for field in fields), (case, left, right)
        if metadata and os.name != "nt":
            assert [v["argv0_base64"] for v in left["vectors"]] == [v["argv0_base64"] for v in right["vectors"]]
        rows.append({"case": case, "go": left, "rust": right, "matches": True,
                     "argv0_equal": [v["argv0_base64"] for v in left["vectors"]] == [v["argv0_base64"] for v in right["vectors"]],
                     "argv0_and_raw_command_line": "raw vectors retained separately for native ownership/argv review; no blanket Windows argv0 equivalence claim"})
    return rows


def script_vectors(probe: Path, owner: Path, tool: str, tools: Path, evidence: RawEvidence) -> list:
    if os.name != "nt": return []
    source = tools / "sdk-oracle.go"; shutil.copyfile(HERE / "discovery_sdk_oracle.go.in", source)
    oracle = tools / "sdk-oracle.exe"
    subprocess.run([tool, "build", "-o", str(oracle), str(source)], env=dict(os.environ, CGO_ENABLED="0", GO111MODULE="off", GOTOOLCHAIN="local"), capture_output=True, timeout=120, check=True)
    rows = []
    for extension, mode in ((extension, mode) for extension in ("bat", "cmd") for mode in ("absolute", "relative-opt-in", "raw-wide")):
        with tempfile.TemporaryDirectory(prefix="bd-script-", dir=key.registry.process.private_temporary_parent()) as folder:
            root = Path(folder); env = key.registry.environment(root); directory = root / ("raw-\ud800" if mode == "raw-wide" else "bin"); directory.mkdir(); marker = root / "shell-was-executed"
            script = directory / ("symvault." + extension)
            script.write_bytes(b'@echo off\r\necho forbidden-shell-owner > "%DISCOVERY_MARKER%"\r\n')
            env.update(PATH="bin" if mode == "relative-opt-in" else str(directory), PATHEXT="." + extension.upper(), NoDefaultCurrentDirectoryInExePath="1", DISCOVERY_MARKER=str(marker), SYMBROWSE_ENCRYPTION_KEY=key.KEY)
            if mode == "relative-opt-in": env["GODEBUG"] = "execerrdot=0"
            actual_go = subprocess.run([str(oracle)], env=env, cwd=root, capture_output=True, timeout=5)
            actual_rust = subprocess.run([str(probe), "symvault", str(owner)], env=env, cwd=root, capture_output=True, timeout=5)
            evidence.append({"phase": "windows.script.observed", "extension": extension, "mode": mode,
                             "native_path_utf16_base64": base64.b64encode(env["PATH"].encode("utf-16le", "surrogatepass")).decode(),
                             "oracle_binary_sha256": key.registry.process.digest(oracle),
                             "probe_binary_sha256": key.registry.process.digest(probe),
                             "go_exit": actual_go.returncode, "rust_exit": actual_rust.returncode,
                             "go_stdout_base64": base64.b64encode(actual_go.stdout).decode(),
                             "go_stderr_base64": base64.b64encode(actual_go.stderr).decode(),
                             "rust_stdout_base64": base64.b64encode(actual_rust.stdout).decode(),
                             "rust_stderr_base64": base64.b64encode(actual_rust.stderr).decode(),
                             "shell_executed": marker.exists()})
            assert actual_go.returncode == actual_rust.returncode == 0 and not actual_go.stderr and not actual_rust.stderr
            go_value, rust_value = json.loads(actual_go.stdout), json.loads(actual_rust.stdout)
            assert not go_value["lookup_error"] and go_value["invoke_error"]
            prefix = 'symvault entry "symbrowse/encryption-key": '
            assert rust_value["error"] == prefix + go_value["invoke_error"] and not rust_value["configured"]
            assert not marker.exists(), "native changed Go failed CreateProcess into shell execution"
            rows.append({"extension": extension, "mode": mode, "native_path_utf16_base64": base64.b64encode(env["PATH"].encode("utf-16le", "surrogatepass")).decode(), "arguments": ["symvault", str(owner)], "go_stdout_base64": base64.b64encode(actual_go.stdout).decode(), "rust_stdout_base64": base64.b64encode(actual_rust.stdout).decode(), "go": go_value, "rust": rust_value, "shell_executed": False, "script_sha256": key.registry.process.digest(script), "oracle_binary_sha256": key.registry.process.digest(oracle), "matches": True})
    return rows


def setting_vectors(probe: Path, owner: Path, provider: Path, tool: str, tools: Path, evidence: RawEvidence) -> list:
    source = tools / "bisect-oracle"; source.mkdir(); package = source / "bisect"; package.mkdir()
    sdk = Path(subprocess.check_output([tool, "env", "GOROOT"], text=True).strip())
    sdk_source = sdk / "src" / "internal" / "bisect" / "bisect.go"
    shutil.copyfile(sdk_source, package / "bisect.go"); shutil.copyfile(sdk / "LICENSE", source / "LICENSE")
    shutil.copyfile(HERE / "discovery_bisect_oracle.go.in", source / "main.go")
    oracle = tools / ("bisect-oracle.exe" if os.name == "nt" else "bisect-oracle-bin")
    subprocess.run([tool, "build", "-o", str(oracle), str(source / "main.go")], cwd=source,
                   env=dict(os.environ, CGO_ENABLED="0", GO111MODULE="off", GOTOOLCHAIN="local"), capture_output=True, timeout=120, check=True)
    supported = [("", True), ("qy", True), ("qn", False), ("q!!y", True), ("q0+1", True),
                 ("qy-y", False), ("q!y-y", True), ("not-a-pattern", True), ("q", True),
                 ("q0-1+0", True), ("qyy", True), ("qxy", True), ("q-0-1", False)]
    supported += [("qxyf", False), ("qxya", False), ("qxyF", False), ("qxy9", False),
                  ("q!xyf", True), ("q!!xyf", False), ("q-xyf", True), ("qy-xyf", True),
                  ("q!y-xyf", False), ("qxyf+y", True), ("qy+xyf", True), ("qxyf+0+1", True),
                  ("q0+1-xyf", True), ("qxyf+y-y", False), ("q-xyf-y", False),
                  ("qxy0", True), ("qxy00", True)]
    unresolved = ["q0", "q1", "qxf", "y", "n", "qy-0", "qvy", "!y"]
    rows = []
    for pattern, expected in supported + [(pattern, None) for pattern in unresolved]:
        with tempfile.TemporaryDirectory(prefix="bd-setting-", dir=key.registry.process.private_temporary_parent()) as folder:
            root = Path(folder); env = key.registry.environment(root); suffix = ".exe" if os.name == "nt" else ""
            install(provider, root / ("symvault" + suffix)); ledger = root / "queries.jsonl"
            path = "."
            if os.sys.platform == "darwin":
                install(provider, root / "keychain" / "security"); path += os.pathsep + str(root / "keychain")
            env.update(PATH=path, GODEBUG="execerrdot=0#" + pattern, NoDefaultCurrentDirectoryInExePath="1",
                       SYMBROWSE_KEY_PROBE_LEDGER=str(ledger), SYMBROWSE_ENCRYPTION_KEY=key.KEY)
            actual_go = subprocess.run([str(oracle), pattern], env=env, cwd=root, capture_output=True, timeout=5)
            actual_rust = subprocess.run([str(probe), "symvault", str(owner)], env=env, cwd=root, capture_output=True, timeout=5)
            raw = {"phase": "setting.observed", "pattern": pattern, "expected_allow": expected,
                   "go_exit": actual_go.returncode, "rust_exit": actual_rust.returncode,
                   "go_stdout_base64": base64.b64encode(actual_go.stdout).decode(), "go_stderr_base64": base64.b64encode(actual_go.stderr).decode(),
                   "rust_stdout_base64": base64.b64encode(actual_rust.stdout).decode(), "rust_stderr_base64": base64.b64encode(actual_rust.stderr).decode(),
                   "ledger_base64": base64.b64encode(ledger.read_bytes()).decode() if ledger.exists() else None,
                   "oracle_binary_sha256": key.registry.process.digest(oracle), "sdk_bisect_source_sha256": key.registry.process.digest(sdk_source)}
            evidence.append(raw)
            assert actual_go.returncode == actual_rust.returncode == 0 and not actual_go.stderr and not actual_rust.stderr
            go_value, rust_value = json.loads(actual_go.stdout), json.loads(actual_rust.stdout)
            queries = [json.loads(line) for line in ledger.read_text().splitlines()] if ledger.exists() else []
            vault_count = sum(query.get("arguments") == ["get", "symbrowse/encryption-key"] for query in queries)
            if expected is None:
                assert not rust_value["configured"] and "requires Go runtime stack/report identity" in rust_value["error"] and vault_count == 0
            else:
                assert all(row["enabled"] == expected and not row["print"] for row in go_value["typed_ids"])
                assert rust_value["configured"] and not rust_value["error"] and vault_count == int(expected)
            rows.append({**raw, "go": go_value, "rust": rust_value,
                         "disposition": "supported quiet/nil-matcher value contract" if expected is not None else "EXPLICIT UNPORTED Go stack/report identity: fail closed, no parity waiver",
                         "vault_queries": vault_count})
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
    binding = {"candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
               "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
               "go_ref": key.registry.process.GO_REF, "go_binary_sha256": key.registry.process.digest(go),
               "rust_binary_sha256": key.registry.process.digest(rust),
               "source_probe_sha256": key.registry.process.digest(args.rust_source_probe.resolve())}
    files = subprocess.check_output(["git", "ls-files", "browse/crates", "browse/port/harness"], cwd=ROOT, text=True).splitlines()
    binding["candidate_source_sha256"] = {p: key.registry.process.digest(ROOT / p) for p in files}
    evidence = RawEvidence(args.out.with_suffix(".raw.json"), binding)
    original = ["empty", "dot", "dot-bin", "relative-bin", "absolute"]
    cases = original + ["relative-first", "absolute-first", "missing-first", "directory-first", "empty-entry-first", "opt-in-dot", "opt-in-relative", "opt-in-empty", "last-opt-in", "last-refusal"]
    cases += ["missing-dotdot", "symlink-dotdot", "relative-missing-dotdot", "suffix-empty", "suffix-quiet-all",
              "suffix-quiet-none", "suffix-last-opt-in", "suffix-last-refusal", "suffix-invalid", "suffix-quiet-partition-all"]
    cases += ["suffix-impossible-hex", "suffix-impossible-lower", "suffix-impossible-upper", "suffix-impossible-digit",
              "suffix-impossible-inverted", "suffix-impossible-double-inverted", "suffix-impossible-subtract",
              "suffix-impossible-prior-all", "suffix-impossible-prior-inverted-all", "suffix-impossible-later-all",
              "suffix-impossible-later-partition", "suffix-impossible-prior-partition", "suffix-impossible-later-none",
              "suffix-invalid-hex"]
    if os.name == "nt": cases += ["implicit-empty", "implicit-absolute-other", "implicit-same", "implicit-hardlink", "implicit-symlink", "pathext-order", "pathext-no-dot", "pathext-empty-list", "raw-wide-path", "raw-wide-pathext"]
    else: cases.append("nonexec-first")
    with tempfile.TemporaryDirectory(prefix="bd-tools-", dir=key.registry.process.private_temporary_parent()) as folder:
        tools = Path(folder); provider_dir = tools / "provider"; provider_dir.mkdir(); provider = build(provider_dir, args.go_tool)
        rows = pairs(go, rust, provider, cases, evidence=evidence, metadata=True)
        scripts = script_vectors(args.rust_source_probe.resolve(), rust, args.go_tool, tools, evidence)
        settings = setting_vectors(args.rust_source_probe.resolve(), rust, provider, args.go_tool, tools, evidence)
        original_rows = pairs(go, rust, args.retained_provider.resolve(), original, evidence=evidence, metadata=False) if args.retained_provider else []
        control_dir = tools / "control"; control_dir.mkdir(); control = build(control_dir, args.go_tool, control=True)
        controls = []
        mutations = [("cwd", "empty-entry-first"), ("suffix-refusal", "suffix-quiet-all")]
        if os.name != "nt": mutations.append(("traversal", "symlink-dotdot"))
        mutations.append(("impossible-mask", "suffix-impossible-hex"))
        for mode, case in mutations:
            mutated = observe(control, rust, provider, case, evidence=evidence, metadata=True, control=True, control_mode=mode)
            ordinary = next(row for row in rows if row["case"] == case)
            assert mutated["vault_query_count"] != ordinary["go"]["vault_query_count"], ("actual discovery mutant escaped", mode)
            controls.append({"mode": mode, "observed": mutated, "binary_sha256": key.registry.process.digest(control), "rejected": True})
        report = {"candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                  "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                  "go_ref": key.registry.process.GO_REF, "go_version": subprocess.check_output([args.go_tool, "version"], text=True).strip(),
                  "go_binary_sha256": key.registry.process.digest(go), "rust_binary_sha256": key.registry.process.digest(rust),
                  "cases": rows, "original_retained_provider_pairs": original_rows, "windows_script_sdk_vectors": scripts,
                  "typed_sdk_setting_vectors": settings, "unported_stack_report_patterns": [row["pattern"] for row in settings if row["expected_allow"] is None],
                  "source_probe_sha256": key.registry.process.digest(args.rust_source_probe), "supported_matches": True, "full_discovery_compatibility": False,
                  "negative_controls": controls,
                  "scope": "actual native default startup CLI owner/query/bytes; every autostart daemon identity stopped and waited before teardown; Linux original10 retained when provided. Windows argv0/rawGetCommandLine remain explicit observations requiring independent review."}
        report["candidate_source_sha256"] = binding["candidate_source_sha256"]
        args.out.write_text(json.dumps(report, indent=2) + "\n")
        evidence.value["complete"] = True; evidence.flush()
    print(f"{len(rows)} actual provider-discovery pairs + {len(original_rows)} original retained pairs; {len(controls)} actual mutants rejected; all owners gone before teardown; Go stack/report bisect compatibility remains explicitly open")
    return 0


if __name__ == "__main__": raise SystemExit(main())
