#!/usr/bin/env python3
"""Actual scoped provider lifetimes; every process belongs to an owned fixture."""
from __future__ import annotations
import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import select
import shutil
import signal
import subprocess
import tempfile
import time
import daemon_state_key as key

ROOT = key.ROOT
INTERNAL = "--internal-startup-key-provider"


class Lease:
    """Hold native process identity where available; never kill by process name."""
    def __init__(self, pid: int, executable: Path):
        assert pid > 0
        self.pid, self.executable, self.handle = pid, executable, None
        if os.name == "nt":
            from ctypes import wintypes
            self.api = ctypes.WinDLL("kernel32", use_last_error=True)
            self.api.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
            self.api.OpenProcess.restype = wintypes.HANDLE
            self.api.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
            self.api.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
            self.api.TerminateProcess.argtypes = [wintypes.HANDLE, wintypes.UINT]
            self.api.CloseHandle.argtypes = [wintypes.HANDLE]
            self.handle = self.api.OpenProcess(0x100001 | 0x1000, False, pid)
            if not self.handle:
                assert ctypes.get_last_error() == 87, ctypes.get_last_error()
                return
            size = wintypes.DWORD(32768); name = ctypes.create_unicode_buffer(size.value)
            assert self.api.QueryFullProcessImageNameW(self.handle, 0, name, ctypes.byref(size))
            assert Path(name.value).resolve() == executable.resolve(), name.value
        elif hasattr(os, "pidfd_open"):
            try: self.handle = os.pidfd_open(pid)
            except ProcessLookupError: return
            try: assert Path(os.readlink(f"/proc/{pid}/exe")) == executable
            except FileNotFoundError: assert not self.alive()

    def alive(self) -> bool:
        if os.name == "nt":
            if not self.handle: return False
            status = self.api.WaitForSingleObject(self.handle, 0)
            assert status in (0, 258), (status, ctypes.get_last_error())
            return status == 258
        if hasattr(os, "pidfd_open"):
            return self.handle is not None and not select.select([self.handle], [], [], 0)[0]
        # Darwin has no pidfd. Only the fixture's recorded PID is inspected.
        result = subprocess.run(["/bin/ps", "-p", str(self.pid), "-o", "stat=", "-o", "comm="], capture_output=True, timeout=2)
        if result.returncode or not result.stdout.strip(): return False
        status, command = result.stdout.decode().strip().split(maxsplit=1)
        if status.startswith("Z"): return False
        assert Path(command).resolve() == self.executable.resolve(), command
        return True

    def cleanup(self):
        if self.alive():
            if os.name == "nt": assert self.api.TerminateProcess(self.handle, 1)
            elif hasattr(signal, "pidfd_send_signal"): signal.pidfd_send_signal(self.handle, signal.SIGKILL)
            else:
                # Bounded fixture lifetime avoids a PID-only kill on Darwin.
                deadline = time.monotonic() + 32
                while self.alive() and time.monotonic() < deadline: time.sleep(.05)
                assert not self.alive(), "owned fixture outlived its hardcoded 30s bound"
        if self.handle is not None:
            if os.name == "nt": self.api.CloseHandle(self.handle)
            else: os.close(self.handle)
            self.handle = None


def queries(path: Path, count: int) -> list[dict]:
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        try: rows = [json.loads(line) for line in path.read_text().splitlines()]
        except (FileNotFoundError, json.JSONDecodeError): rows = []
        if len(rows) >= count:
            assert len(rows) == count, rows
            assert all(row["name"] == "symvault" and isinstance(row["pid"], int) for row in rows)
            return rows
        time.sleep(.01)
    raise AssertionError("owned provider did not record its actual processes")


def wait_gone(leases: list[Lease]) -> float:
    begin = time.monotonic()
    while time.monotonic() - begin < 2:
        if not any(lease.alive() for lease in leases): return time.monotonic() - begin
        time.sleep(.01)
    raise AssertionError("startup cancellation left an owned provider or descendant alive")


def observe(binary: Path, provider: Path, mode: str, trigger: str) -> dict:
    with tempfile.TemporaryDirectory(prefix="bk-own-", dir=key.registry.process.private_temporary_parent()) as directory:
        root = Path(directory); env = key.registry.environment(root)
        owned_bin = root / "providers"; owned_bin.mkdir(mode=0o700)
        executable = owned_bin / ("symvault.exe" if os.name == "nt" else "symvault")
        shutil.copyfile(provider, executable); executable.chmod(0o700)
        ledger = root / "queries.jsonl"
        env.update(PATH=str(owned_bin), SYMBROWSE_KEY_PROBE_MODE=mode,
                   SYMBROWSE_KEY_PROBE_LEDGER=str(ledger), SYMBROWSE_ENCRYPTION_KEY=key.KEY)
        count = 2 if mode in ("descendant", "pipe-holder") else 1
        session = "own" + str(os.getpid())
        if trigger == "client-deadline": command = [str(binary), "state", "list", "--session", session, "--json"]
        elif trigger == "daemon-signal": command = [str(binary), "daemon", "--session", session, "--json"]
        else: command = [str(binary), INTERNAL, str(executable), "100" if trigger == "helper-deadline" else "15000", "get", "symbrowse/encryption-key"]
        child = subprocess.Popen(command, cwd=root, env=env, stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 start_new_session=(os.name == "posix"))
        leases = []; unrelated = None; held_writer_alive = []
        begin = time.monotonic()
        try:
            rows = queries(ledger, count)
            leases = [Lease(row["pid"], executable) for row in rows]
            initial = [lease.alive() for lease in leases]
            if trigger in ("closed-writer", "held-writer"):
                assert initial[-1], initial
                # A genuine same-executable sibling is outside this lease.
                sibling_env = dict(env, SYMBROWSE_KEY_PROBE_MODE="timeout")
                sibling_env.pop("SYMBROWSE_KEY_PROBE_LEDGER")
                unrelated = subprocess.Popen([str(executable)], env=sibling_env,
                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    start_new_session=(os.name == "posix"))
                if trigger == "held-writer":
                    time.sleep(.1)
                    held_writer_alive = [lease.alive() for lease in leases]
                    assert all(held_writer_alive), "held-writer control was falsely accepted as cleanup"
                child.stdin.close(); child.stdin = None
            elif trigger == "daemon-signal":
                if os.name == "posix": child.send_signal(signal.SIGTERM)
                else: child.kill()  # Windows has no portable SIGTERM shutdown.
            # Keep the lifetime writer open while observing a normal/deadline
            # completion. The fixtures emit less than one pipe buffer.
            child.wait(timeout=8)
            stdout, stderr = child.communicate(timeout=2)
            elapsed = time.monotonic() - begin
            gone_elapsed = wait_gone(leases)
            sibling_survived = unrelated is None or unrelated.poll() is None
            assert sibling_survived, "startup cancellation affected a separate owned provider"
            if trigger == "client-deadline":
                assert child.returncode == 1 and 4.5 <= elapsed <= 7
                envelope = json.loads(stdout); assert envelope["error"]["code"] == "daemon_unavailable" and not stderr
            elif trigger != "daemon-signal":
                assert child.returncode == 0
                result = json.loads(stderr)
                if mode == "ab":
                    assert result["result"] == "Exited" and result["status"] == 0
                    assert json.loads(stdout)["value"] == key.KEY
                elif mode == "4":
                    assert result == {"result": "Exited", "status": 4 if os.name == "nt" else 4 << 8}
                    assert not stdout
                else:
                    assert result["result"] == "Failed" and ("timed out" in result["message"] or result["message"] == "stdout pipe remained open after command exit") and not stdout
            return {"mode": mode, "trigger": trigger, "root": str(root), "arguments": command,
                    "exit": child.returncode, "stdout": stdout.decode(), "stderr": stderr.decode(),
                    "queries": rows, "initial_alive": initial, "provider_and_descendants_gone": True,
                    "elapsed_seconds": elapsed, "cleanup_seconds": gone_elapsed,
                    "unrelated_provider_survived": sibling_survived,
                    "held_writer_alive": held_writer_alive,
                    "held_writer_control_rejected": trigger == "held-writer" and any(held_writer_alive)}
        finally:
            key.registry.harness.kill_tree(child)
            if unrelated is not None: key.registry.harness.kill_tree(unrelated)
            for lease in leases: lease.cleanup()


def unavailable(binary: Path) -> dict:
    with tempfile.TemporaryDirectory(prefix="bk-own-missing-", dir=key.registry.process.private_temporary_parent()) as root:
        command = [str(binary), INTERNAL, str(Path(root) / ("symvault.exe" if os.name == "nt" else "symvault")), "1000", "get", "symbrowse/encryption-key"]
        result = subprocess.run(command, input=b"", capture_output=True,
                                start_new_session=(os.name == "posix"), timeout=3)
        assert result.returncode == 0 and not result.stdout and json.loads(result.stderr) == {"result": "Unavailable"}
        return {"arguments": command, "exit": result.returncode, "stdout": result.stdout.decode(), "stderr": result.stderr.decode()}


def refusals(binary: Path, provider: Path) -> list[dict]:
    rows = []
    with tempfile.TemporaryDirectory(prefix="bk-own-refusal-", dir=key.registry.process.private_temporary_parent()) as directory:
        root = Path(directory); env = key.registry.environment(root)
        executable = root / ("symvault.exe" if os.name == "nt" else "symvault")
        shutil.copyfile(provider, executable); executable.chmod(0o700)
        ledger = root / "queries.jsonl"
        env.update(SYMBROWSE_KEY_PROBE_MODE="ab", SYMBROWSE_KEY_PROBE_LEDGER=str(ledger))
        cases = [[str(executable), "0", "get", "symbrowse/encryption-key"],
                 [str(executable), "60001", "get", "symbrowse/encryption-key"],
                 [str(executable), "bad", "get", "symbrowse/encryption-key"],
                 [str(executable), "1000", "set", "symbrowse/encryption-key"],
                 [str(executable), "1000", "get", "other-entry"],
                 [str(executable), "1000", "get", "symbrowse/encryption-key", "--print"],
                 [str(executable), "1000"], []]
        for values in cases:
            result = subprocess.run([str(binary), INTERNAL, *values], input=b"", env=env,
                                    capture_output=True, start_new_session=(os.name == "posix"), timeout=2)
            assert result.returncode == 1 and not result.stdout and not result.stderr
            assert not ledger.exists(), "invalid helper route queried the provider"
            rows.append({"arguments": values, "exit": result.returncode,
                         "stdout": result.stdout.decode(), "stderr": result.stderr.decode(), "provider_not_queried": True})
        if os.name == "posix":
            result = subprocess.run([str(binary), INTERNAL, str(executable), "1000", "get", "symbrowse/encryption-key"],
                                    input=b"", env=env, capture_output=True, timeout=2)
            assert result.returncode == 1 and not result.stdout and not result.stderr and not ledger.exists()
            rows.append({"name": "refuse-inherited-unix-group", "exit": result.returncode, "provider_not_queried": True})
    return rows


def source_boundaries(probe: Path, binary: Path, provider: Path, go: str) -> dict:
    with tempfile.TemporaryDirectory(prefix="bk-owner-api-", dir=key.registry.process.private_temporary_parent()) as directory:
        root = Path(directory); env = key.registry.environment(root)
        executable = root / ("symvault.exe" if os.name == "nt" else "symvault")
        security = root / "security"
        for path in (executable, security): shutil.copyfile(provider, path); path.chmod(0o700)
        control_source = root / "owner-control.go"
        shutil.copyfile(key.HERE / "state_key_control.go.in", control_source)
        control = root / ("owner-control.exe" if os.name == "nt" else "owner-control")
        subprocess.run([go, "build", "-o", str(control), str(control_source)],
                       env=dict(os.environ, CGO_ENABLED="0", GOTOOLCHAIN="local", GO111MODULE="off"),
                       capture_output=True, check=True, timeout=120)
        ledger = root / "queries.jsonl"
        env.update(PATH=str(root), SYMBROWSE_KEY_PROBE_MODE="4", SYMBROWSE_KEYCHAIN_PROBE_MODE="44",
                   SYMBROWSE_KEY_PROBE_LEDGER=str(ledger), SYMBROWSE_ENCRYPTION_KEY=key.KEY)
        rows = []
        for name, owner, marker, mode in (("missing-supervisor", root / "missing-owner", "", "4"),
                    ("malformed-supervisor", control, "malformed", "4"),
                    ("honest-denied-provider", binary, "", "4"),
                    ("legitimate-provider-absence", binary, "", "3"),
                    ("false-absence-control", control, "unavailable", "4")):
            ledger.unlink(missing_ok=True)
            actual_env = dict(env, SYMBROWSE_STARTUP_OWNER_CONTROL=marker, SYMBROWSE_KEY_PROBE_MODE=mode)
            result = subprocess.run([str(probe), str(executable), str(owner)], env=actual_env,
                                    cwd=root, capture_output=True, timeout=5)
            assert result.returncode == 0 and not result.stderr
            observation = json.loads(result.stdout)
            recorded = [json.loads(line) for line in ledger.read_text().splitlines()] if ledger.exists() else []
            if name == "missing-supervisor":
                assert observation == {"configured": False, "key_source": "", "error": 'symvault entry "symbrowse/encryption-key": startup provider supervisor unavailable'} and not recorded
            elif name == "malformed-supervisor":
                assert observation == {"configured": False, "key_source": "", "error": 'symvault entry "symbrowse/encryption-key": invalid startup provider result'} and not recorded
            elif name == "honest-denied-provider":
                assert observation == {"configured": False, "key_source": "", "error": 'symvault entry "symbrowse/encryption-key": exit status 4'} and len(recorded) == 1
            else:
                assert observation == {"configured": True, "key_source": "environment", "error": ""}
                if name == "legitimate-provider-absence": assert len(recorded) == (2 if platform.system() == "Darwin" else 1)
                else: assert not recorded
            rows.append({"name": name, "arguments": result.args, "exit": result.returncode,
                         "stdout": result.stdout.decode(), "stderr": result.stderr.decode(), "queries": recorded,
                         "control_rejected": name == "false-absence-control" and observation["configured"]})
        return {"probe_binary_sha256": key.registry.process.digest(probe),
                "control_binary_sha256": key.registry.process.digest(control), "root": str(root), "cases": rows}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--rust-source-probe", type=Path, required=True)
    parser.add_argument("--go-tool", default="go")
    args = parser.parse_args(); binary = args.rust.resolve()
    rows = []
    with tempfile.TemporaryDirectory(prefix="bk-own-tools-", dir=key.registry.process.private_temporary_parent()) as root:
        provider = key.key_test_environment.build_provider(Path(root), args.go_tool)
        provider_sha = key.registry.process.digest(provider)
        refusal_rows = refusals(binary, provider)
        boundaries = source_boundaries(args.rust_source_probe.resolve(), binary, provider, args.go_tool)
        for mode, trigger in (("ab", "normal"), ("4", "normal"), ("descendant", "closed-writer"),
                              ("pipe-holder", "closed-writer"), ("descendant", "helper-deadline"),
                              ("descendant", "daemon-signal"), ("descendant", "client-deadline"),
                              ("descendant", "held-writer")):
            row = observe(binary, provider, mode, trigger); rows.append(row)
            print(mode, trigger, "owned processes gone", flush=True)
    sources = subprocess.check_output(["git", "-C", str(ROOT), "ls-files", "browse/crates", "browse/port/harness"], text=True).splitlines()
    report = {"candidate_head": subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip(),
              "candidate_dirty": bool(subprocess.check_output(["git", "-C", str(ROOT), "status", "--porcelain"])),
              "platform": platform.platform(), "rust_binary_sha256": key.registry.process.digest(binary),
              "provider_binary_sha256": provider_sha, "provider_source_sha256": key.registry.process.digest(key.HERE / "state_key_fixture.go.in"),
              "candidate_source_sha256": {name: key.registry.process.digest(ROOT / name) for name in sources},
              "cases": rows, "source_boundaries": boundaries, "refusals": refusal_rows, "unavailable": unavailable(binary), "matches": True,
              "scope": "explicit Browse CLI startup only; same-name sibling survives; no operator providers; native host evidence only"}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
