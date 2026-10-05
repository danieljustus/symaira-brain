#!/usr/bin/env python3
"""Run affected tests with owned absence providers and no operator key material."""
from __future__ import annotations
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent

def build_provider(root: Path, go: str = "go") -> Path:
    source = root / "key-provider.go"
    shutil.copyfile(HERE / "state_key_fixture.go.in", source)
    executable = root / ("key-provider.exe" if os.name == "nt" else "key-provider")
    env = dict(os.environ, CGO_ENABLED="0", GOTOOLCHAIN="local", GO111MODULE="off")
    subprocess.run([go, "build", "-o", str(executable), str(source)], env=env,
                   capture_output=True, timeout=120, check=True)
    return executable

def absent_providers(root: Path, go: str = "go") -> Path:
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    provider = build_provider(root, go)
    owned_bin = root / "bin"
    owned_bin.mkdir(mode=0o700)
    for name in ("symvault", "security"):
        target = owned_bin / (name + (".exe" if os.name == "nt" else ""))
        shutil.copyfile(provider, target)
        target.chmod(0o700)
    return owned_bin

def test_environment(root: Path, *, go: str = "go") -> dict[str, str]:
    env = {key: value for key, value in os.environ.items() if not key.startswith("SYMBROWSE_")}
    # Preserve toolchain identity while state, browser profiles, and providers
    # are scoped to an owned root. Owned names precede every inherited command.
    env["CARGO_HOME"] = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))
    env["RUSTUP_HOME"] = os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup"))
    owned_bin = absent_providers(root / "providers", go)
    home = root / "home"; home.mkdir(mode=0o700)
    for key, suffix in (("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"),
                        ("XDG_DATA_HOME", "data"), ("XDG_STATE_HOME", "state"),
                        ("XDG_RUNTIME_DIR", "run"), ("LOCALAPPDATA", "Local")):
        env[key] = str(root / suffix)
    env.update(HOME=str(home), USERPROFILE=str(home),
               PATH=str(owned_bin) + os.pathsep + env.get("PATH", ""),
               SYMBROWSE_KEY_PROBE_MODE="3", SYMBROWSE_KEYCHAIN_PROBE_MODE="44")
    return env

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--go", default="go")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command: parser.error("a test command is required")
    # Darwin direct execution uses the existing external-volume policy.
    import daemon_process
    with tempfile.TemporaryDirectory(prefix="bk-test-", dir=daemon_process.private_temporary_parent()) as owned:
        return subprocess.run(command, env=test_environment(Path(owned), go=args.go)).returncode

if __name__ == "__main__":
    raise SystemExit(main())
