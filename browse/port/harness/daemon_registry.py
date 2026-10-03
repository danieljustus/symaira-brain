#!/usr/bin/env python3
"""Source-bound registry and real CLI autostart observations in private roots."""
from __future__ import annotations

import argparse
import base64
import concurrent.futures
import copy
from datetime import datetime
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("process", HERE / "daemon_process.py")
process = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(process)
harness = process.harness
CLI_SPEC = importlib.util.spec_from_file_location("registry_cli", HERE / "daemon_registry_cli.py")
cli_edges = importlib.util.module_from_spec(CLI_SPEC)
CLI_SPEC.loader.exec_module(cli_edges)


def environment(root: Path) -> dict:
    home = root / "home"
    home.mkdir(mode=0o700)
    runtime = home / "Library/Caches/symbrowse/run" if sys.platform == "darwin" else root / "run"
    runtime.mkdir(parents=True, mode=0o700)
    temp = root / "temp"
    temp.mkdir(mode=0o700)
    env = {key: os.environ[key] for key in ("SystemRoot", "windir", "ComSpec", "PATHEXT") if key in os.environ}
    env.update(HOME=str(home), USERPROFILE=str(home), LOCALAPPDATA=str(root / "Local"),
               XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
               XDG_STATE_HOME=str(root / "state"), XDG_DATA_HOME=str(root / "data"),
               XDG_RUNTIME_DIR=str(runtime), TMPDIR=str(temp), TMP=str(temp), TEMP=str(temp), PATH=process.absent_keychain_path(root))
    return env


def cli(binary: Path, root: Path, env: dict, arguments: list[str]) -> dict:
    result = subprocess.run([str(binary), *arguments], cwd=root, env=env,
                            capture_output=True, timeout=15)
    return {"arguments": arguments, "exit": result.returncode,
            "stdout": result.stdout.decode(), "stderr": result.stderr.decode()}


def start(binary: Path, root: Path, env: dict, session: str):
    child = subprocess.Popen([str(binary), "daemon", "--session", session], cwd=root, env=env,
                             stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             start_new_session=(os.name == "posix"))
    endpoint = harness.daemon_socket_path(Path(env["XDG_RUNTIME_DIR"]), session)
    try:
        harness.wait_for_request(endpoint, {"cmd": "daemon.ping", "session": session})
    except Exception:
        harness.kill_tree(child)
        raise
    return child, endpoint


def stop(child, endpoint: Path, session: str) -> dict:
    try:
        response = harness.request(endpoint, {"cmd": "daemon.stop", "session": session})
        stdout, stderr = child.communicate(timeout=10)
        if child.returncode != 0 or stdout or response != {"success": True, "data": {"stopping": True}}:
            raise AssertionError(f"daemon stop failed: {child.returncode}, {stdout!r}, {stderr!r}")
        return {"exit": child.returncode, "stdout": stdout.decode(), "stderr": stderr.decode()}
    finally:
        harness.kill_tree(child)


def observe(binary: Path, session: str, fixtures: dict[str, str]) -> dict:
    with tempfile.TemporaryDirectory(prefix="br-", dir=process.private_temporary_parent()) as temporary:
        root = Path(temporary)
        env = environment(root)
        began = time.time()
        missing = []
        # Inspection never starts a daemon, even when autostart is permitted.
        for disabled in (False, True):
            if disabled:
                env["SYMBROWSE_NO_AUTOSTART"] = "1"
            for command in ("session list", "session info", "daemon status", "daemon stop"):
                for output in ([], ["--json"], ["--output", "yaml"]):
                    missing.append(cli(binary, root, env, [*command.split(), "--session", session, *output]))
        missing.append(cli(binary, root, env, ["state", "list", "--session", session, "--json"]))
        env["SYMBROWSE_NO_AUTOSTART"] = "1"
        before_invalid = sorted(p.relative_to(root).as_posix() for p in root.rglob("*"))
        invalid_cli = [cli(binary, root, env, [*command.split(), "--session", name, *output])
                       for command in ("session list", "session info", "daemon status", "state list")
                       for name in ("bad session", "bad\x1bsession", "bad\u0301session", "bad\u00adsession")
                       for output in ([], ["--json"], ["--output", "yaml"])]
        assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*")) == before_invalid, "invalid CLI created daemon/profile/state files"
        extra_cli = cli_edges.observe(binary, root, env)
        child, endpoint = start(binary, root, env, session)
        frames = [{"cmd": "session.info"}, {"cmd": "session.info", "session": "unknown"}]
        frames += [{"cmd": "daemon.ping", "session": name} for name in
                   ("zulu", "alpha", "a" * 64, "bad session!", "_invalid", "../escape", "é", "a" * 65,
                    "bad\0session", "bad\x1bsession", "bad\u0301session", "bad\u00adsession")]
        frames += [{"cmd": "session.list", "session": "zulu"},
                   {"cmd": "session.list", "session": "alpha"},
                   {"cmd": "session.info", "session": "alpha"}]
        records = []
        profile = None
        try:
            for frame in frames:
                time.sleep(.005)
                records.append({"request": frame, "response": harness.request(endpoint, frame)})
            listed = records[-2]["response"]["data"]["sessions"]
            before_touch = next(s for s in records[-3]["response"]["data"]["sessions"] if s["name"] == "alpha")
            after_touch = next(s for s in listed if s["name"] == "alpha")
            assert datetime.fromisoformat(after_touch["last_activity"].replace("Z", "+00:00")) > datetime.fromisoformat(before_touch["last_activity"].replace("Z", "+00:00"))
            assert [s["name"] for s in listed] == sorted([session, "zulu", "alpha", "a" * 64])
            profile = Path(listed[0]["user_data_dir"])
            for info in listed:
                path = Path(info["user_data_dir"])
                if not path.is_relative_to(root) or not path.is_dir():
                    raise AssertionError(f"profile escaped owned root: {path}")
                if os.name == "posix" and path.stat().st_mode & 0o777 != 0o700:
                    raise AssertionError("session profile was not secured")
            (profile / "retained").write_text("owned-profile-marker")
            inspected = [cli(binary, root, env, ["session", command, "--session", session, *output])
                         for command in ("list", "info")
                         for output in ([], ["--json"], ["--output", "yaml"])]
            first_pid = child.pid
            first_stop = stop(child, endpoint, session)
            child, endpoint = start(binary, root, env, session)
            restarted = harness.request(endpoint, {"cmd": "session.list", "session": session})
            assert [s["name"] for s in restarted["data"]["sessions"]] == [session]
            assert (profile / "retained").read_text() == "owned-profile-marker"
            restart_pid = child.pid
            restart_stop = stop(child, endpoint, session)
        finally:
            harness.kill_tree(child)
        # Exercise real concurrent CLI children, configured logs and default argv.
        config = root / "config/symbrowse"
        config.mkdir(parents=True, mode=0o700)
        state = root / "configured-state"
        config.joinpath("config.toml").write_text(f"state_dir = {json.dumps(state.as_posix())}\nread_timeout = 7\n")
        env.pop("SYMBROWSE_NO_AUTOSTART")
        auto_session = session + "a"
        auto_endpoint = harness.daemon_socket_path(Path(env["XDG_RUNTIME_DIR"]), auto_session)
        try:
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                clients = list(pool.map(lambda _: cli(binary, root, env,
                    ["state", "list", "--session", auto_session, "--json"]), range(8)))
            owner = harness.request(auto_endpoint, {"cmd": "daemon.status", "session": auto_session})
            info = harness.request(auto_endpoint, {"cmd": "session.info", "session": auto_session})
            assert owner["data"]["pid"] == info["data"]["pid"]
            log = state / "daemon.log"
            assert log.is_file(), "autostart ignored the resolved configured state directory"
            if os.name == "posix":
                assert log.stat().st_mode & 0o777 == 0o600
                assert state.stat().st_mode & 0o777 == 0o700
            state_files = state / "states"
            for name, raw in fixtures.items():
                path = state_files / (name + ".json")
                path.write_bytes(bytes.fromhex(raw))
                path.chmod(0o600)
            state_commands = []
            for arguments in (["list"], ["show", "alpha"], ["show", "missing"],
                              ["clear", "zulu"], ["clean"], ["clean", "--older-than", "1"], ["list"]):
                for output in ([], ["--json"], ["--output", "yaml"]):
                    observed = cli(binary, root, env, ["state", *arguments, "--session", auto_session, *output])
                    if "private-owned-value" in observed["stdout"] + observed["stderr"]:
                        raise AssertionError("metadata inspection leaked a state value")
                    state_commands.append(observed)
            assert (state_files / "alpha.json").read_bytes().hex() == fixtures["alpha"], "metadata/clean rewrote retained migration data"
            assert sorted(p.name for p in state_files.iterdir()) == ["alpha.json"], "clear/clean removed the wrong state files"
        finally:
            try:
                harness.request(auto_endpoint, {"cmd": "daemon.stop", "session": auto_session})
            except OSError:
                pass
        return {"root": str(root), "session": session, "begin": began, "end": time.time(),
                "pid": first_pid, "restart_pid": restart_pid, "missing": missing, "invalid_cli": invalid_cli,
                "cli_edges": extra_cli,
                "registry": records, "inspection": inspected, "first_stop": first_stop,
                "restart_stop": restart_stop, "restart": restarted,
                "autostart": clients, "owner": owner, "owner_info": info, "state_commands": state_commands}


def normalize(record: dict, *, rust: bool) -> dict:
    value = copy.deepcopy(record)
    value.pop("cli_edges")  # compare() verifies literal bytes before this projection.
    root, begin, end = value.pop("root"), value.pop("begin"), value.pop("end")
    pids = {value["pid"], value["restart_pid"], value["owner"]["data"]["pid"]}
    assert all(isinstance(pid, int) and pid > 0 for pid in pids)
    assert len(pids) == 3, "restart/autostart did not have independent process owners"
    owner_pid = value["owner"]["data"]["pid"]
    first_pid, restart_pid = value["pid"], value["restart_pid"]
    for record in value["registry"]:
        data = record["response"].get("data", {})
        infos = data.get("sessions", [data])
        for info in infos:
            if "pid" in info:
                assert info["pid"] == first_pid, "registry reported a different process owner"
    for info in value["restart"]["data"]["sessions"]:
        assert info["pid"] == restart_pid, "restart retained the previous process owner"
    for key in ("owner", "owner_info"):
        assert value[key]["data"]["pid"] == owner_pid
    value.pop("pid")
    value.pop("restart_pid")
    for key in ("first_stop", "restart_stop"):
        stop_result = value.pop(key)
        assert stop_result["exit"] == 0 and stop_result["stdout"] == ""
        if rust:
            assert stop_result["stderr"] == "symbrowse shutdown: begin\nsymbrowse shutdown: no browser session\n"
        else:
            line = stop_result["stderr"]
            match = re.fullmatch(r'time=([^ ]+) level=WARN msg="no external risk decider available \(symbrain not found or disabled\); risk decisions fall back to the built-in policy"\n', line)
            assert match, f"unrecognized Go diagnostic: {line!r}"
            timestamp = datetime.fromisoformat(match[1].replace("Z", "+00:00")).timestamp()
            assert begin - 1 <= timestamp <= end + 1
    if rust:
        status = value["owner"]["data"]
        assert status.pop("session") == value["session"] + "a"
        assert status.pop("mode") == "browser"

    def walk(item, key=""):
        if isinstance(item, dict):
            return {k: walk(v, k) for k, v in item.items()}
        if isinstance(item, list):
            return [walk(v, key) for v in item]
        if key == "pid":
            assert item in pids, f"foreign PID {item}"
            return "<validated-owner-pid>"
        if key in ("started_at", "last_activity"):
            assert item.endswith("Z"), f"activity timestamp is not UTC: {item}"
            timestamp = datetime.fromisoformat(item.replace("Z", "+00:00")).timestamp()
            assert begin - 1 <= timestamp <= end + 1, f"timestamp outside observation: {item}"
            return "<validated-time>"
        if isinstance(item, str):
            return item.replace(root, "<owned-root>").replace(root.replace("\\", "/"), "<owned-root>")
        return item

    def text_fields(text, expected_pid=None):
        def timestamp(match):
            raw = match[2].strip("\"'")
            parsed = datetime.fromisoformat(raw.replace("Z", "+00:00")).timestamp()
            assert begin - 1 <= parsed <= end + 1, f"invalid text timestamp: {raw}"
            return match[1] + "<validated-time>"
        def pid(match):
            numeric = float(match[2])
            assert numeric.is_integer() and int(numeric) in pids, f"foreign text PID: {match[2]}"
            if expected_pid is not None:
                assert int(numeric) == expected_pid, "inspection text reported a different owner"
            return match[1] + "<validated-owner-pid>"
        text = re.sub(r"(?m)^(\s*(?:started_at|last_activity):\s*)([^\n]+)$", timestamp, text)
        return re.sub(r"(?m)^(\s*pid:\s*)([0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)$", pid, text)

    for group in ("missing", "invalid_cli", "inspection", "autostart", "state_commands"):
        for observed in value[group]:
            failed = group in ("missing", "invalid_cli") or group == "state_commands" and "missing" in observed["arguments"]
            assert observed["exit"] == (1 if failed else 0), observed
            if not failed:
                assert not observed["stderr"], observed
            if observed["stdout"].lstrip().startswith("{"):
                observed["stdout"] = json.loads(observed["stdout"])
                if group == "inspection":
                    data = observed["stdout"].get("data", observed["stdout"])
                    for info in data.get("sessions", [data]):
                        assert info["pid"] == first_pid, "inspection reported a different process owner"
            else:
                observed["stdout"] = text_fields(observed["stdout"], first_pid if group == "inspection" else None)
    return walk(value)


def compare(case: dict) -> bool:
    # These generic validation failures have no paths/PIDs/timestamps: preserve
    # literal arguments, exit, stdout and stderr without any projection.
    assert case["go"]["invalid_cli"] == case["rust"]["invalid_cli"], "invalid-session CLI bytes differ"
    cli_edges.compare(case["go"]["cli_edges"], case["rust"]["cli_edges"])
    return normalize(case["go"], rust=False) == normalize(case["rust"], rust=True)


def controls(case: dict) -> list[dict]:
    rejected = []
    for name in ("foreign-owner", "previous-owner-after-restart", "missing-error-detail", "invalid-timestamp", "invalid-cli-exit", "invalid-cli-protocol-code", "raw-cli-byte-loss", "help-description"):
        bad = copy.deepcopy(case)
        if name == "foreign-owner":
            bad["rust"]["owner_info"]["data"]["pid"] += 1
        elif name == "previous-owner-after-restart":
            bad["rust"]["restart"]["data"]["sessions"][0]["pid"] = bad["rust"]["pid"]
        elif name == "missing-error-detail":
            bad["rust"]["missing"][1]["stdout"] = '{"success":false,"error":{"code":"daemon_unavailable"}}'
        elif name == "invalid-cli-exit":
            bad["rust"]["invalid_cli"][0]["exit"] = 7
        elif name == "invalid-cli-protocol-code":
            response = json.loads(bad["rust"]["invalid_cli"][1]["stdout"])
            response["error"]["code"] = "invalid_session"
            bad["rust"]["invalid_cli"][1]["stdout"] = json.dumps(response)
        elif name in ("raw-cli-byte-loss", "help-description"):
            group = "invalid" if name == "raw-cli-byte-loss" else "help"
            record = bad["rust"]["cli_edges"][group][3 if group == "invalid" else 0]
            field = "stderr_base64" if group == "invalid" else "stdout_base64"
            raw = base64.b64decode(record[field])
            raw = raw.replace(b"\\xff", "\ufffd".encode()) if group == "invalid" and os.name == "posix" else raw + b" changed diagnostic"
            if group == "help":
                raw = base64.b64decode(record[field]).replace(b"Inspect browser sessions", b"Inspect daemon-owned sessions")
            record[field] = base64.b64encode(raw).decode()
        else:
            bad["rust"]["registry"][0]["response"]["data"]["started_at"] = "1970-01-01T00:00:00Z"
        try:
            matched = compare(bad)
        except AssertionError:
            matched = False
        assert not matched, f"negative control escaped comparator: {name}"
        rejected.append({"name": name, "rejected": True})
    return rejected


def oracle_api(source: Path) -> dict:
    with tempfile.TemporaryDirectory(prefix="br-api-", dir=process.private_temporary_parent()) as directory:
        root = Path(directory)
        overlay = root / "overlay.json"
        overlay.write_text(json.dumps({"Replace": {
            str(source / "browse/internal/daemon/registry_port_supplemental_test.go"):
                str(HERE / "registry_oracle_test.go.in")}}))
        env = dict(os.environ)
        env.update(CGO_ENABLED="0", GOTELEMETRY="off")
        version = subprocess.check_output(["go", "version"], text=True).strip()
        result = subprocess.run(["go", "test", "-mod=readonly", "-overlay", str(overlay), "./internal/daemon",
            "-run", "^TestPortRegistryAPISupplemental$", "-count=1", "-v"],
            cwd=source / "browse", env=env, capture_output=True, timeout=300)
        stdout, stderr = result.stdout.decode(), result.stderr.decode()
        if result.returncode or "--- PASS: TestPortRegistryAPISupplemental" not in stdout:
            raise AssertionError(f"frozen Go supplemental API gate failed: {result.returncode}\n{stdout}\n{stderr}")
        observation = next(line.partition("=")[2] for line in stdout.splitlines()
                           if line.startswith("REGISTRY_API_OBSERVATIONS="))
        return {"exit": result.returncode, "go_version": version, "stdout": stdout, "stderr": stderr,
                "observations": json.loads(observation), "probe_sha256": process.digest(HERE / "registry_oracle_test.go.in")}


def main() -> int:
    parser = argparse.ArgumentParser()
    for name in ("go", "rust", "go-source", "out"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    source = args.go_source.resolve()
    assert subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip() == process.GO_REF
    assert not subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    api = oracle_api(source)
    assert api["observations"]["scalar_quote_sha256"] == "4c752b4c6e90df8c641d6ac02a6da80113943bc8fc476c825f27aef51db2a055", "pinned Go scalar quoting changed"
    assert not subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True), "oracle API modified frozen Go"
    session = f"r{os.getpid()}"
    fixtures = api["observations"]["state_fixtures_hex"]
    case = {"go": observe(args.go.resolve(), session, fixtures), "rust": observe(args.rust.resolve(), session, fixtures)}
    args.out.with_suffix(".raw.json").write_text(json.dumps(case, indent=2) + "\n")
    case["matches"] = compare(case)
    root = HERE.parents[2]
    files = subprocess.check_output(["git", "-C", str(root), "ls-files", "browse/crates", "browse/port/harness"], text=True).splitlines()
    go_files = subprocess.check_output(["git", "-C", str(source), "ls-files", "browse"], text=True).splitlines()
    report = {"candidate_head": subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip(),
              "candidate_dirty": bool(subprocess.check_output(["git", "-C", str(root), "status", "--porcelain"], text=True)),
              "go_ref": process.GO_REF, "platform": platform.platform(),
              "go_binary_sha256": process.digest(args.go), "rust_binary_sha256": process.digest(args.rust),
              "candidate_source_sha256": {f: process.digest(root / f) for f in files},
              "go_source_sha256": {f: process.digest(source / f) for f in go_files},
              "counts_per_binary": {"cli_observations": 108 + len(case["go"]["cli_edges"]["invalid"]) + 4 + 6, "invalid_session_cli_observations": 48,
                                    "literal_cli_edge_observations": len(case["go"]["cli_edges"]["invalid"]), "selected_invalid_output_observations": 4, "implemented_help_observations": 6, "recorded_raw_frames": 20,
                                    "concurrent_clients": 8, "persisted_go_fixtures": 3},
              "case": case, "oracle_api": api, "matches": case["matches"], "negative_controls": controls(case)}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    if not case["matches"]:
        left, right = normalize(case["go"], rust=False), normalize(case["rust"], rust=True)
        for key in left:
            if left[key] != right[key]:
                print(f"difference in {key}: Go={left[key]!r}; Rust={right[key]!r}")
    print(f"{report['counts_per_binary']['cli_observations']} CLI observations (48 original invalid + {len(case['go']['cli_edges']['invalid'])} literal edge + 4 selected-output errors + 6 help) + 20 raw frames + 8 concurrent clients per binary: matches={case['matches']}; 8 controls rejected")
    return 0 if case["matches"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
