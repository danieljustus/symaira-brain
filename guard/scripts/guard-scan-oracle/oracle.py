#!/usr/bin/env python3
"""Freeze symbrain guard scan bytes from one pinned Go revision."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "scripts"))
from external_env import ensure_external_environment as ensure_shared_external_environment

FIXTURE = HERE / "fixture.json"
PINNED_COMMIT_SHA = "9c0e2b259753901a372ed5a688382bb6d4fadd18"
# The pin covers the Go sources whose scan behaviour this oracle freezes. It
# deliberately excludes go.mod and go.sum: those change on every dependency
# update, so binding them made the check fail by construction on routine
# Dependabot Go-module bumps, which touch nothing else. The subject here is the
# scan implementation, not the dependency graph — the module files are still
# pinned by go.sum's own hashes and by the Rust/Go dependency gates.
GO_SOURCE_PATHS = (
    "cmd/symbrain/cmd_guard.go",
    "guard/cmd/symguard/scan/command.go",
    "guard/internal/discovery/discovery.go",
    "guard/internal/discovery/result.go",
    "guard/internal/discovery/secrets.go",
    "guard/internal/output/reporter.go",
)
GO_TOOLCHAIN = "go1.26.7"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run(argv: list[str], *, cwd: Path, env: dict[str, str], timeout: float = 180.0) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(argv, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, check=False)


def git_show(path: str, commit: str = PINNED_COMMIT_SHA) -> bytes:
    return subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=ROOT)


def require_pin() -> dict[str, str]:
    try:
        subprocess.run(["git", "cat-file", "-e", f"{PINNED_COMMIT_SHA}^{{commit}}"], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
        expected = {path: sha256(git_show(path)) for path in GO_SOURCE_PATHS}
    except (OSError, subprocess.CalledProcessError) as exc:
        raise RuntimeError(f"pinned Go revision unavailable: {PINNED_COMMIT_SHA}") from exc
    return expected


def verify_binding(document: dict[str, object]) -> dict[str, str]:
    expected = require_pin()
    if document.get("source_commit") != PINNED_COMMIT_SHA:
        raise RuntimeError("fixture is not bound to the pinned Go revision")
    if document.get("source_files") != expected:
        raise RuntimeError("fixture source hashes do not match the pinned Go revision")
    current = {path: sha256((ROOT / path).read_bytes()) for path in GO_SOURCE_PATHS}
    if current != expected:
        raise RuntimeError("working-tree Go scan sources drifted from the pinned revision")
    return expected


def config_files(root: Path, *, missing_hermes: bool = False, unknown_opencode: bool = True) -> tuple[Path, Path, dict[str, str]]:
    home = root / "home"
    xdg = root / "xdg"
    files = {
        "hermes": home / ".config/hermes/config.json",
        "cursor": home / ".cursor/mcp.json",
        "vscode": home / ".vscode/mcp.json",
        "opencode": home / ".config/opencode/config.json",
        "claude": (home / "Library/Application Support/Claude/claude_desktop_config.json")
        if sys.platform == "darwin"
        else xdg / "claude/claude_desktop_config.json",
    }
    contents = {
        "hermes": b'''{
  // JSONC proves comments are accepted without changing URL strings.
  "mcpServers": {"jsonc-command": {"command": "node", "args": ["server.js"], "env": {"TOKEN": "secret-jsonc"}}}
}
''',
        "cursor": b'''mcpServers:
  yaml-command:
    command: python
    args: [server.py]
    env:
      YAML_TOKEN: secret-yaml
''',
        "vscode": b'{"mcpServers":{"url-server":{"url":"https://example.test/mcp"}}}\n',
        "opencode": b'{"mcp":{"local":{"type":"local","command":"uvx","args":["mcp-server"],"env":{"BASE":"old","SHARED":"env"},"environment":{"BASE":"new","ONLY_ENVIRONMENT":"secret"}},"remote":{"type":"remote","url":"https://example.test/remote"},"unknown":{"type":"mystery","command":"mystery"}}}\n',
        "claude": b'{"mcpServers":{"claude-command":{"command":"deno","args":["run.ts"]}}}\n',
    }
    if not unknown_opencode:
        contents["opencode"] = contents["opencode"].replace(b'"type":"mystery"', b'"type":"local"')
    for name, path in files.items():
        if missing_hermes and name == "hermes":
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(contents[name])
    return home, xdg, {name: str(path) for name, path in files.items()}


def case_env(root: Path, *, missing_hermes: bool = False, unknown_opencode: bool = True) -> dict[str, str]:
    home, xdg, _ = config_files(root, missing_hermes=missing_hermes, unknown_opencode=unknown_opencode)
    env = {"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C", "TZ": "UTC"}
    env.update({
        "HOME": str(home),
        "XDG_CONFIG_HOME": str(xdg),
        "XDG_DATA_HOME": str(root / "data"),
        "XDG_CACHE_HOME": str(root / "cache"),
        "TMPDIR": str(root / "tmp"),
    })
    for name in ("data", "cache", "tmp"):
        (root / name).mkdir(parents=True, exist_ok=True)
    return env


def canonical(data: bytes, root: Path) -> bytes:
    replacements = {
        str(root / "home"): "/oracle-home",
        str(root / "xdg"): "/oracle-config",
        str(root / "data"): "/oracle-data",
        str(root / "cache"): "/oracle-cache",
        str(root / "tmp"): "/oracle-tmp",
    }
    for old, new in replacements.items():
        data = data.replace(old.encode(), new.encode())
    return data


def run_pipe(binary: Path, args: list[str], env: dict[str, str], cwd: Path) -> tuple[int, bytes, bytes]:
    proc = run([str(binary), "guard", "scan", *args], cwd=cwd, env=env, timeout=15)
    return proc.returncode, proc.stdout, proc.stderr


def run_tty(binary: Path, args: list[str], env: dict[str, str], cwd: Path) -> tuple[int, bytes, bytes]:
    if os.name == "nt":
        raise RuntimeError("the pseudoterminal oracle case is POSIX-only")
    import pty
    import select
    import tty

    master, slave = pty.openpty()
    try:
        tty.setraw(slave)
        proc = subprocess.Popen([str(binary), "guard", "scan", *args], cwd=cwd, env=env, stdout=slave, stderr=subprocess.PIPE)
        os.close(slave)
        slave = -1
        stdout = bytearray()
        while True:
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    break
                if chunk:
                    stdout.extend(chunk)
            if proc.poll() is not None:
                break
        while True:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            if not chunk:
                break
            stdout.extend(chunk)
        proc.wait(timeout=15)
        return proc.returncode, bytes(stdout), proc.stderr.read()
    finally:
        if slave != -1:
            os.close(slave)
        os.close(master)


def encoded(data: bytes) -> dict[str, object]:
    return {"base64": base64.b64encode(data).decode("ascii"), "bytes": len(data), "sha256": sha256(data)}


def run_scan_case(binary: Path, source: Path, root: Path, case_id: str, args: list[str], *, tty_mode: bool = False, missing_hermes: bool = False, unknown_opencode: bool = True, repeat: int = 1) -> dict[str, object]:
    env = case_env(root, missing_hermes=missing_hermes, unknown_opencode=unknown_opencode)
    runs = []
    for _ in range(repeat):
        rc, stdout, stderr = (run_tty if tty_mode else run_pipe)(binary, args, env, source)
        runs.append((rc, canonical(stdout, root), canonical(stderr, root)))
    if any(item != runs[0] for item in runs[1:]):
        raise RuntimeError(f"case {case_id} changed bytes across repeated runs")
    rc, stdout, stderr = runs[0]
    return {
        "id": case_id,
        "args": args,
        "tty": tty_mode,
        "repeat": repeat,
        "exit_code": rc,
        "stdout": encoded(stdout),
        "stderr": encoded(stderr),
    }


def matrix(source: Path, env: dict[str, str]) -> dict[str, object]:
    helper = source / "scan-oracle-matrix.go"
    helper.write_text(
        '''package main
import ("encoding/json"; "fmt"; "os"; "github.com/danieljustus/symaira-corekit/mcpcfgkit")
func main() { os.Setenv("XDG_CONFIG_HOME", "/oracle-config"); out := map[string]any{}
 out["darwin"] = mcpcfgkit.DefaultSourcesForPlatform("darwin", "/oracle-home")
 out["linux"] = mcpcfgkit.DefaultSourcesForPlatform("linux", "/oracle-home")
 b, err := json.MarshalIndent(out, "", "  "); if err != nil { panic(err) }; fmt.Println(string(b)) }
''',
        encoding="utf-8",
    )
    try:
        proc = run(["go", "run", str(helper)], cwd=source, env=env, timeout=60)
        if proc.returncode != 0:
            raise RuntimeError(f"platform/XDG matrix failed: {proc.stderr.decode(errors='replace')}")
        return json.loads(proc.stdout)
    finally:
        helper.unlink(missing_ok=True)


def fixture_for_platform(document: dict[str, object], *, windows: bool) -> dict[str, object]:
    if not windows:
        return document
    projected = dict(document)
    projected["cases"] = [case for case in document["cases"] if not case.get("tty", False)]
    projected["case_count"] = len(projected["cases"])
    return projected


def build_and_run() -> dict[str, object]:
    expected = require_pin()
    runtime_parent = Path(os.environ.get("SYMAIRA_EXTERNAL_RUNTIME_ROOT", tempfile.gettempdir()))
    runtime = runtime_parent / "guard-scan-oracle-runtime"
    shutil.rmtree(runtime, ignore_errors=True)
    runtime.mkdir(parents=True)
    source = runtime / "source"
    binary = runtime / ("symbrain-go.exe" if os.name == "nt" else "symbrain-go")
    try:
        subprocess.run(["git", "worktree", "add", "--quiet", "--detach", str(source), PINNED_COMMIT_SHA], cwd=ROOT, check=True)
        build_env = dict(os.environ)
        build_env.update({"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(runtime / "build-home"), "GOTOOLCHAIN": GO_TOOLCHAIN, "CGO_ENABLED": "0", "TMPDIR": str(runtime / "build-tmp")})
        (runtime / "build-home").mkdir()
        (runtime / "build-tmp").mkdir()
        build = run(["go", "build", "-o", str(binary), "./cmd/symbrain"], cwd=source, env=build_env)
        if build.returncode != 0:
            raise RuntimeError(f"pinned Go build failed: {build.stderr.decode(errors='replace')}")
        root = runtime / "case-base"
        root.mkdir()
        cases = [
            run_scan_case(binary, source, root, "default-pipe-json", [], repeat=2),
        ]
        # Python's stdlib has no Windows PTY support; retain all pipe cases.
        if os.name != "nt":
            cases.append(run_scan_case(binary, source, root, "default-tty-table", [], tty_mode=True))
        cases.extend([
            run_scan_case(binary, source, root, "explicit-json", ["--format", "json"]),
            run_scan_case(binary, source, root, "explicit-table", ["--format", "table"]),
            run_scan_case(binary, source, root, "help", ["--help"]),
            run_scan_case(binary, source, root, "error-unknown-argument", ["--bogus"]),
            run_scan_case(binary, source, root, "error-format-missing", ["--format"]),
            run_scan_case(binary, source, root, "error-format-unsupported", ["--format", "xml"]),
            run_scan_case(binary, source, runtime / "case-missing", "one-finding-missing-hermes", ["--format", "json"], missing_hermes=True, unknown_opencode=False, repeat=3),
        ])
        matrix_env = dict(build_env)
        matrix_env.update({"XDG_CONFIG_HOME": "/oracle-config"})
        return {
            "schema_version": 1,
            "source_commit": PINNED_COMMIT_SHA,
            "source_files": expected,
            "case_count": len(cases),
            "cases": cases,
            "platform_xdg_sources": matrix(source, matrix_env),
        }
    finally:
        subprocess.run(["git", "worktree", "remove", "--force", str(source)], cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        shutil.rmtree(runtime, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=("write", "check"))
    args = parser.parse_args()
    ensure_shared_external_environment(__file__)
    if args.action == "check":
        current = json.loads(FIXTURE.read_text(encoding="utf-8"))
        verify_binding(current)
    generated = build_and_run()
    if args.action == "check":
        expected = fixture_for_platform(current, windows=os.name == "nt")
        if json.dumps(expected, indent=2, sort_keys=True) + "\n" != json.dumps(generated, indent=2, sort_keys=True) + "\n":
            raise RuntimeError("guard scan oracle fixture drifted; run guard/scripts/guard-scan-oracle/run.sh write after changing the pin")
        suffix = "; POSIX TTY case excluded" if os.name == "nt" else ""
        print(f"PASS: guard scan oracle byte check passed ({generated['case_count']} cases{suffix})")
    else:
        FIXTURE.write_text(json.dumps(generated, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"Wrote {FIXTURE} ({generated['case_count']} cases)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        print(f"guard-scan-oracle: {exc}", file=os.sys.stderr)
        raise SystemExit(1)
