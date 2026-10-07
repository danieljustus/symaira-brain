#!/usr/bin/env python3
"""Source-bound startup/keyed-store observations against real immutable Go."""
from __future__ import annotations
import argparse
import copy
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import time
import daemon_registry as registry
import key_test_environment

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FROZEN = ROOT / "browse/testdata/port/state"
MANIFEST = json.loads((FROZEN / "manifest.json").read_text())
KEY = MANIFEST["key_hex"]
OTHER = "cd" * 32


def inputs() -> list[dict]:
    cases = [{"name": "no-key", "key": None, "vault": None},
             {"name": "empty-key", "key": "", "vault": None},
             {"name": "environment-ab", "key": KEY, "vault": None},
             {"name": "environment-wrong-cd", "key": OTHER, "vault": None},
             {"name": "environment-trimmed-ab", "key": " " + KEY + " \n", "vault": None}]
    for output, name in (([], "text"), (["--json"], "json"), (["--output", "yaml"], "yaml")):
        cases.append({"name": "invalid-environment-" + name, "key": "not-a-key", "vault": None, "output": output})
    for mode in ("ab", "cd", "2", "3", "4", "invalid", "empty", "timeout"):
        cases.append({"name": "vault-" + mode + "-environment-ab", "key": OTHER if mode == "ab" else KEY, "vault": mode})
    cases.append({"name": "state-directory-is-file", "key": None, "vault": None, "store_file": True})
    # Original seventeen remain additive. Actual argv order/format failures
    # prove the startup adapter does not turn a denied provider into readiness.
    for output, label in ((["--json"], "json"), (["--output", "yaml"], "yaml"),
                          (["--output", "invalid", "--json"], "json-last"),
                          (["--json", "--output", "invalid"], "json-first")):
        cases.append({"name": "vault-denied-" + label, "key": KEY, "vault": "4", "output": output})
    for key, label in (("zz" * 32, "nonhex64"), ("ab" * 31, "short"), ("ab" * 33, "long"),
                       ("\u2003" + KEY + "\u2003", "unicode-space")):
        cases.append({"name": "environment-" + label, "key": key, "vault": None})
    cases += [{"name": "vault-3-no-environment", "key": None, "vault": "3"},
              {"name": "vault-empty-no-environment", "key": None, "vault": "empty"},
              {"name": "environment-internal-space", "key": KEY[:32] + "\u2003" + KEY[32:], "vault": None}]
    for layout in ("forged-none", "corrupt-later", "authenticated-nonjson"):
        cases.append({"name": layout, "key": KEY, "vault": None, "layout": layout})
    if platform.system() == "Darwin":
        for mode in ("ab", "cd", "44", "4", "invalid", "empty", "missing"):
            cases.append({"name": "keychain-" + mode, "key": OTHER if mode == "ab" else KEY,
                          "vault": None, "keychain": mode})
    return cases


def oracle_api(source: Path, go: str) -> dict:
    with tempfile.TemporaryDirectory(prefix="bk-api-", dir=registry.process.private_temporary_parent()) as directory:
        root = Path(directory)
        overlay = root / "overlay.json"
        overlay.write_text(json.dumps({"Replace": {
            str(source / "browse/internal/state/port_state_key_supplemental_test.go"):
                str(HERE / "state_key_oracle_test.go.in")}}))
        env = dict(os.environ, CGO_ENABLED="0", GOTELEMETRY="off", GOTOOLCHAIN="local")
        result = subprocess.run([go, "test", "-mod=readonly", "-overlay", str(overlay),
                                 "./internal/state", "-run", "^TestPortStateKeyStoreObservation$", "-count=1", "-v"],
                                cwd=source / "browse", env=env, capture_output=True, timeout=120)
        stdout, stderr = result.stdout.decode(), result.stderr.decode()
        assert result.returncode == 0 and "--- PASS: TestPortStateKeyStoreObservation" in stdout, (stdout, stderr)
        values = {line.partition("=")[0]: line.partition("=")[2] for line in stdout.splitlines() if line.startswith("STATE_KEY_")}
        assert "WARN re-saving encrypted state without an encryption key state=existing previous_key_source=environment" in stdout + stderr
        return {"exit": result.returncode, "stdout": stdout, "stderr": stderr,
                "observations": json.loads(values["STATE_KEY_STORE_OBSERVATIONS"]),
                "authenticated_nonjson_hex": values["STATE_KEY_AUTHENTICATED_NONJSON_HEX"],
                "probe_sha256": registry.process.digest(HERE / "state_key_oracle_test.go.in")}


def observe(binary: Path, case: dict, provider: Path, api: dict, control: tuple[Path, str] | None = None) -> dict:
    with tempfile.TemporaryDirectory(prefix="bk-", dir=registry.process.private_temporary_parent()) as directory:
        root = Path(directory)
        env = registry.environment(root)
        env["SYMBROWSE_NO_AUTOSTART"] = "1"
        session = "key" + str(os.getpid())
        ledger = root / "queries.jsonl"
        owned_bin = root / "bin"; owned_bin.mkdir(mode=0o700)
        if case["vault"] is not None:
            target = owned_bin / ("symvault.exe" if os.name == "nt" else "symvault")
            shutil.copyfile(provider, target); target.chmod(0o700)
        if platform.system() == "Darwin" and case.get("keychain") != "missing":
            target = owned_bin / "security"
            shutil.copyfile(provider, target); target.chmod(0o700)
        env.update(PATH=str(owned_bin), SYMBROWSE_KEY_PROBE_MODE=case["vault"] or "",
                   SYMBROWSE_KEYCHAIN_PROBE_MODE=case.get("keychain", "44"),
                   SYMBROWSE_KEY_PROBE_LEDGER=str(ledger))
        if case["key"] is not None: env["SYMBROWSE_ENCRYPTION_KEY"] = case["key"]
        states = Path(env["XDG_STATE_HOME"]) / "symbrowse/states"
        states.mkdir(parents=True, mode=0o700)
        for fixture in MANIFEST["cases"]:
            shutil.copyfile(FROZEN / fixture["path"], states / (fixture["name"] + ".json"))
        if case.get("layout"):
            for path in states.iterdir(): path.unlink()
            valid = (FROZEN / "encrypted-v3.state").read_bytes()
            damaged = valid
            if case["layout"] == "forged-none":
                magic = b"SYMBROWSE-STATE\x00"
                header, body = valid[len(magic):].split(b"\n", 1)
                decoded = json.loads(header); decoded["key_source"] = "none"
                damaged = magic + json.dumps(decoded, separators=(",", ":")).encode() + b"\n" + body
            elif case["layout"] == "corrupt-later":
                damaged = valid[:-1] + bytes([valid[-1] ^ 1])
            elif case["layout"] == "authenticated-nonjson":
                damaged = bytes.fromhex(api["authenticated_nonjson_hex"])
            for name, raw in (("a-valid", valid), ("b-checked", damaged), ("c-later", valid)):
                (states / (name + ".json")).write_bytes(raw)
        if case.get("store_file"):
            shutil.rmtree(states); states.write_text("owned-blocking-file")
        endpoint = registry.harness.daemon_socket_path(Path(env["XDG_RUNTIME_DIR"]), session)
        command = [str(binary), "daemon", "--session", session, *case.get("output", [])]
        if control is not None:
            command = [str(control[0]), control[1], *command]
        began = time.monotonic()
        child = subprocess.Popen(command, cwd=root, env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 start_new_session=(os.name == "posix"))
        ready = False; records = []; before = after = retained = {}; stopped = None
        try:
            deadline = began + 22
            while child.poll() is None and time.monotonic() < deadline:
                try:
                    ready = registry.harness.request(endpoint, {"cmd": "daemon.ping", "session": session}).get("success", False)
                    if ready: break
                except OSError: pass
                time.sleep(.02)
            elapsed = time.monotonic() - began
            if ready:
                before = {path.name: registry.process.digest(path) for path in states.iterdir()}
                frames = [{"cmd": "state.list", "session": session}]
                if not case.get("layout"):
                    frames += [{"cmd": "state.show", "session": session, "args": {"name": fixture["name"]}}
                               for fixture in MANIFEST["cases"]]
                    # A repeated encrypted access must reuse the startup decision.
                    frames.append({"cmd": "state.show", "session": session, "args": {"name": "encrypted-v3"}})
                if case.get("layout"):
                    frames.append({"cmd": "state.show", "session": session, "args": {"name": "b-checked"}})
                for frame in frames:
                    records.append({"request": frame, "response": registry.harness.request(endpoint, frame)})
                after = {path.name: registry.process.digest(path) for path in states.iterdir()}
                assert before == after, "read-only metadata changed persisted bytes"
                frame = {"cmd": "state.clean", "session": session}
                records.append({"request": frame, "response": registry.harness.request(endpoint, frame)})
                retained = {path.name: registry.process.digest(path) for path in states.iterdir()}
                stopped = registry.harness.request(endpoint, {"cmd": "daemon.stop", "session": session})
            if child.poll() is None and not ready:
                raise AssertionError("startup exceeded owned bound")
            stdout, stderr = child.communicate(timeout=10)
            for secret in ("fixture-secret", KEY, OTHER):
                assert secret not in (stdout + stderr).decode()
                assert secret not in json.dumps(records)
            if not ready:
                try: registry.harness.request(endpoint, {"cmd": "daemon.ping", "session": session})
                except OSError: pass
                else: raise AssertionError("failed key resolution exposed an IPC endpoint")
            queries = [json.loads(line) for line in ledger.read_text().splitlines()] if ledger.exists() else []
            assert len([query for query in queries if query["name"] == "symvault"]) <= 1
            assert len([query for query in queries if query["name"] == "security"]) <= 1
            if case["vault"] == "timeout":
                assert 14 <= elapsed <= 22 and not ready, "provider deadline was bypassed"
            return {"root": str(root), "arguments": command, "startup_ready": ready,
                    "startup_elapsed_seconds": elapsed, "exit": child.returncode,
                    "stdout": stdout.decode(), "stderr": stderr.decode(), "records": records,
                    "queries": queries, "retained_before": before, "retained_after_inspection": after,
                    "retained_after_clean": retained, "stop": stopped}
        finally:
            registry.harness.kill_tree(child)


def normalized(observation: dict) -> dict:
    # Compare every protocol field, literal failed-start stdout/stderr and all
    # retained file hashes. Only owned paths and validated provider PIDs vary.
    value = {field: copy.deepcopy(observation[field]) for field in
             ("startup_ready", "exit", "stdout", "records", "retained_before",
              "retained_after_inspection", "retained_after_clean", "stop")}
    if not observation["startup_ready"]: value["stderr"] = observation["stderr"]
    queries = []
    for query in observation["queries"]:
        assert isinstance(query["pid"], int) and query["pid"] > 0
        queries.append({"name": query["name"], "arguments": query["arguments"]})
    value["queries"] = queries
    # Root replacement is limited to each runner's recorded owned prefix.
    def owned(value):
        if isinstance(value, str):
            # JSON-formatted diagnostics may escape a Windows root inside the
            # literal stdout string. Replace only that same recorded prefix.
            return value.replace(json.dumps(observation["root"])[1:-1], "<OWNED_ROOT>").replace(observation["root"], "<OWNED_ROOT>")
        if isinstance(value, dict): return {key: owned(child) for key, child in value.items()}
        if isinstance(value, list): return [owned(child) for child in value]
        return value
    return owned(value)


def main() -> int:
    parser = argparse.ArgumentParser()
    for name in ("go", "rust", "go-source", "out"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--go-tool", default="go")
    parser.add_argument("--rust-store-probe", type=Path, required=True)
    args = parser.parse_args()
    source = args.go_source.resolve()
    assert subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip() == registry.process.GO_REF
    assert not subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    for fixture in MANIFEST["cases"]:
        assert registry.process.digest(FROZEN / fixture["path"]) == fixture["sha256"]
    api = oracle_api(source, args.go_tool)
    rows = []
    with tempfile.TemporaryDirectory(prefix="bk-tools-", dir=registry.process.private_temporary_parent()) as directory:
        provider = key_test_environment.build_provider(Path(directory), args.go_tool)
        provider_digest = registry.process.digest(provider)
        control_source = Path(directory) / "key-control.go"
        shutil.copyfile(HERE / "state_key_control.go.in", control_source)
        control_binary = Path(directory) / ("key-control.exe" if os.name == "nt" else "key-control")
        subprocess.run([args.go_tool, "build", "-o", str(control_binary), str(control_source)],
                       env=dict(os.environ, CGO_ENABLED="0", GOTOOLCHAIN="local", GO111MODULE="off"),
                       capture_output=True, check=True, timeout=120)
        control_digest = registry.process.digest(control_binary)
        for case in inputs():
            go, rust = observe(args.go.resolve(), case, provider, api), observe(args.rust.resolve(), case, provider, api)
            row = {"input": case, "go": go, "rust": rust, "matches": normalized(go) == normalized(rust)}
            rows.append(row)
            print(case["name"], "matches=", row["matches"], flush=True)
        controls = []
        for mode, case_name in (("wrong-environment", "environment-ab"),
                                ("wrong-vault", "vault-ab-environment-ab"),
                                ("denied-to-absence", "vault-4-environment-ab")):
            row = next(row for row in rows if row["input"]["name"] == case_name)
            actual = observe(args.rust.resolve(), row["input"], provider, api, (control_binary, mode))
            rejected = normalized(actual) != normalized(row["go"])
            controls.append({"name": mode, "input": row["input"], "actual": actual,
                             "wrapper_binary_sha256": control_digest, "rejected": rejected})
            assert rejected, f"actual control was falsely accepted: {mode}"
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.with_suffix(".raw.json").write_text(json.dumps({"oracle_api": api, "cases": rows, "negative_controls": controls}, indent=2) + "\n")
    with tempfile.TemporaryDirectory(prefix="bk-store-", dir=registry.process.private_temporary_parent()) as directory:
        result = subprocess.run([str(args.rust_store_probe.resolve()), str(Path(directory) / "new-store")], capture_output=True, timeout=15)
        store_probe = {"exit": result.returncode, "stdout": result.stdout.decode(), "stderr": result.stderr.decode(),
                       "binary_sha256": registry.process.digest(args.rust_store_probe)}
        assert result.returncode == 0
        store_probe["observations"] = json.loads(result.stdout)
        assert store_probe["observations"] == api["observations"]
        assert 'WARN re-saving encrypted state without an encryption key state="existing" previous_key_source="environment"' in result.stderr.decode()
    assert not subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    files = subprocess.check_output(["git", "-C", str(ROOT), "ls-files", "browse/crates", "browse/port/harness"], text=True).splitlines()
    go_files = subprocess.check_output(["git", "-C", str(source), "ls-files", "browse"], text=True).splitlines()
    report = {"candidate_head": subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip(),
              "candidate_dirty": bool(subprocess.check_output(["git", "-C", str(ROOT), "status", "--porcelain"])),
              "go_ref": registry.process.GO_REF, "platform": platform.platform(),
              "go_version": subprocess.check_output([args.go_tool, "version"], text=True).strip(),
              "binaries_sha256": {"go": registry.process.digest(args.go), "rust": registry.process.digest(args.rust), "provider": provider_digest},
              "candidate_source_sha256": {file: registry.process.digest(ROOT / file) for file in files},
              "go_source_sha256": {file: registry.process.digest(source / file) for file in go_files},
              "historical_manifest_sha256": registry.process.digest(FROZEN / "manifest.json"),
              "oracle_api": api, "rust_store_probe": store_probe, "negative_controls": controls,
              "cases": rows, "startup_pairs": len(rows), "matches": all(row["matches"] for row in rows)}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["matches"] else 1

if __name__ == "__main__":
    raise SystemExit(main())
