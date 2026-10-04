#!/usr/bin/env python3
"""Source-bound registry and real CLI autostart observations in private roots."""
from __future__ import annotations

import argparse
import concurrent.futures
from datetime import datetime
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time
from registry_progress import Progress, event
from registry_compare import compare, controls, normalize

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


def cli(binary: Path, root: Path, env: dict, arguments: list[str], progress=None) -> dict:
    case = progress.begin_cli(binary, arguments) if progress is not None else None
    result = subprocess.run([str(binary), *arguments], cwd=root, env=env,
                            capture_output=True, timeout=15)
    if progress is not None: progress.end_cli(case, result)
    return {"arguments": arguments, "exit": result.returncode,
            "stdout": result.stdout.decode(), "stderr": result.stderr.decode()}


def start(binary: Path, root: Path, env: dict, session: str, progress=None):
    event(progress, "daemon.start.begin", session=session, binary=str(binary))
    child = subprocess.Popen([str(binary), "daemon", "--session", session], cwd=root, env=env,
                             stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             start_new_session=(os.name == "posix"))
    endpoint = harness.daemon_socket_path(Path(env["XDG_RUNTIME_DIR"]), session)
    event(progress, "daemon.ready.begin", session=session, pid=child.pid, endpoint=str(endpoint))
    try:
        harness.wait_for_request(endpoint, {"cmd": "daemon.ping", "session": session})
    except Exception:
        harness.kill_tree(child)
        raise
    event(progress, "daemon.ready.end", session=session, pid=child.pid)
    return child, endpoint



def request(endpoint: Path, frame: dict, progress=None) -> dict:
    event(progress, "frame.begin", endpoint=str(endpoint), request=frame)
    response = harness.request(endpoint, frame)
    event(progress, "frame.end", endpoint=str(endpoint), request=frame)
    return response


def stop(child, endpoint: Path, session: str, progress=None) -> dict:
    event(progress, "daemon.stop.begin", session=session, pid=child.pid)
    try:
        response = request(endpoint, {"cmd": "daemon.stop", "session": session}, progress)
        stdout, stderr = child.communicate(timeout=10)
        if child.returncode != 0 or stdout or response != {"success": True, "data": {"stopping": True}}:
            raise AssertionError(f"daemon stop failed: {child.returncode}, {stdout!r}, {stderr!r}")
        event(progress, "daemon.stop.end", session=session, pid=child.pid, exit=child.returncode)
        return {"exit": child.returncode, "stdout": stdout.decode(), "stderr": stderr.decode()}
    finally:
        harness.kill_tree(child)


def observe(binary: Path, session: str, fixtures: dict[str, str], progress=None) -> dict:
    with tempfile.TemporaryDirectory(prefix="br-", dir=process.private_temporary_parent()) as temporary:
        root = Path(temporary)
        env = environment(root)
        def run_cli(arguments):
            return cli(binary, root, env, arguments, progress)
        event(progress, "observe.begin", binary=str(binary), root=str(root), session=session)
        began = time.time()
        missing = []
        # Inspection never starts a daemon, even when autostart is permitted.
        for disabled in (False, True):
            if disabled:
                env["SYMBROWSE_NO_AUTOSTART"] = "1"
            for command in ("session list", "session info", "daemon status", "daemon stop"):
                for output in ([], ["--json"], ["--output", "yaml"]):
                    missing.append(run_cli([*command.split(), "--session", session, *output]))
        missing.append(run_cli(["state", "list", "--session", session, "--json"]))
        env["SYMBROWSE_NO_AUTOSTART"] = "1"
        before_invalid = sorted(p.relative_to(root).as_posix() for p in root.rglob("*"))
        invalid_cli = [run_cli([*command.split(), "--session", name, *output])
                       for command in ("session list", "session info", "daemon status", "state list")
                       for name in ("bad session", "bad\x1bsession", "bad\u0301session", "bad\u00adsession")
                       for output in ([], ["--json"], ["--output", "yaml"])]
        assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*")) == before_invalid, "invalid CLI created daemon/profile/state files"
        event(progress, "cli.edges.begin")
        extra_cli = cli_edges.observe(binary, root, env, progress)
        event(progress, "cli.edges.end")
        child, endpoint = start(binary, root, env, session, progress)
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
                records.append({"request": frame, "response": request(endpoint, frame, progress)})
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
            inspected = [run_cli(["session", command, "--session", session, *output])
                         for command in ("list", "info")
                         for output in ([], ["--json"], ["--output", "yaml"])]
            first_pid = child.pid
            first_stop = stop(child, endpoint, session, progress)
            child, endpoint = start(binary, root, env, session, progress)
            event(progress, "restart.frame.begin", session=session)
            restarted = request(endpoint, {"cmd": "session.list", "session": session}, progress)
            event(progress, "restart.frame.end", session=session)
            assert [s["name"] for s in restarted["data"]["sessions"]] == [session]
            assert (profile / "retained").read_text() == "owned-profile-marker"
            restart_pid = child.pid
            restart_stop = stop(child, endpoint, session, progress)
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
            event(progress, "autostart.begin", session=auto_session)
            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                clients = list(pool.map(lambda _: run_cli(
                    ["state", "list", "--session", auto_session, "--json"]), range(8)))
            event(progress, "autostart.owner.begin", session=auto_session)
            owner = request(auto_endpoint, {"cmd": "daemon.status", "session": auto_session}, progress)
            info = request(auto_endpoint, {"cmd": "session.info", "session": auto_session}, progress)
            event(progress, "autostart.owner.end", session=auto_session)
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
                    observed = run_cli(["state", *arguments, "--session", auto_session, *output])
                    if "private-owned-value" in observed["stdout"] + observed["stderr"]:
                        raise AssertionError("metadata inspection leaked a state value")
                    state_commands.append(observed)
            assert (state_files / "alpha.json").read_bytes().hex() == fixtures["alpha"], "metadata/clean rewrote retained migration data"
            assert sorted(p.name for p in state_files.iterdir()) == ["alpha.json"], "clear/clean removed the wrong state files"
        finally:
            try:
                event(progress, "autostart.stop.begin", session=auto_session)
                request(auto_endpoint, {"cmd": "daemon.stop", "session": auto_session}, progress)
                event(progress, "autostart.stop.end", session=auto_session)
            except OSError:
                pass
        event(progress, "observe.end", binary=str(binary), session=session)
        return {"root": str(root), "session": session, "begin": began, "end": time.time(),
                "pid": first_pid, "restart_pid": restart_pid, "missing": missing, "invalid_cli": invalid_cli,
                "cli_edges": extra_cli,
                "registry": records, "inspection": inspected, "first_stop": first_stop,
                "restart_stop": restart_stop, "restart": restarted,
                "autostart": clients, "owner": owner, "owner_info": info, "state_commands": state_commands}


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
    progress = Progress(args.out.with_suffix(".progress.jsonl"), {
        "candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=HERE.parents[2], text=True).strip(),
        "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=HERE.parents[2])),
        "journal_source_sha256": process.digest(HERE / "registry_progress.py"),
        "registry_source_sha256": process.digest(Path(__file__)),
        "cli_source_sha256": process.digest(HERE / "daemon_registry_cli.py"),
        "go_ref": process.GO_REF, "go_binary_sha256": process.digest(args.go),
        "rust_binary_sha256": process.digest(args.rust), "scope": "diagnostic only; full assertions unchanged"})
    event(progress, "oracle.api.begin")
    api = oracle_api(source)
    event(progress, "oracle.api.end")
    assert api["observations"]["scalar_quote_sha256"] == "4c752b4c6e90df8c641d6ac02a6da80113943bc8fc476c825f27aef51db2a055", "pinned Go scalar quoting changed"
    assert not subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True), "oracle API modified frozen Go"
    session = f"r{os.getpid()}"
    fixtures = api["observations"]["state_fixtures_hex"]
    case = {"go": observe(args.go.resolve(), session, fixtures, progress), "rust": observe(args.rust.resolve(), session, fixtures, progress)}
    args.out.with_suffix(".raw.json").write_text(json.dumps(case, indent=2) + "\n")
    event(progress, "comparison.begin")
    case["matches"] = compare(case)
    event(progress, "comparison.end", matches=case["matches"])
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
    event(progress, "receipt.complete", matches=case["matches"])
    print(f"{report['counts_per_binary']['cli_observations']} CLI observations (48 original invalid + {len(case['go']['cli_edges']['invalid'])} literal edge + 4 selected-output errors + 6 help) + 20 raw frames + 8 concurrent clients per binary: matches={case['matches']}; 8 controls rejected")
    return 0 if case["matches"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
