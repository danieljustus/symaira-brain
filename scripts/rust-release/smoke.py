#!/usr/bin/env python3
"""Smoke-test one extracted release archive on its native platform.

  smoke.py --dir extracted --version V
  smoke.py --archive-dir dist --version V   # extracts the single archive in dist

Checks that `symbrain version` reports V, and that a command the Rust CLI
hands to its Go fallback reaches the packaged `symbrain-go` through PATH and
produces exactly the Go oracle's exit code, stdout and stderr. The environment
is minimal and owned: an empty HOME/XDG tree and no SYMBRAIN_GO_BINARY.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

FALLBACK_ARGS = ["skills", "targets", "--candidate-smoke-unknown"]


def environment(candidate: Path, scratch: Path) -> dict[str, str]:
    system = (
        [str(Path(os.environ.get("SystemRoot", r"C:\Windows")) / "System32")]
        if os.name == "nt"
        else ["/usr/bin", "/bin"]
    )
    env = {"PATH": os.pathsep.join([str(candidate), *system])}
    for name in ["home", "config", "data", "cache"]:
        (scratch / name).mkdir()
    env.update(HOME=str(scratch / "home"), USERPROFILE=str(scratch / "home"),
               XDG_CONFIG_HOME=str(scratch / "config"), XDG_DATA_HOME=str(scratch / "data"),
               XDG_CACHE_HOME=str(scratch / "cache"))
    if os.name == "nt":
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"]:
            if key in os.environ:
                env[key] = os.environ[key]
    return env


def run(binary: Path, args: list[str], env: dict[str, str], cwd: Path) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run([str(binary), *args], env=env, cwd=cwd, capture_output=True, timeout=60)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--dir", type=Path, help="already extracted archive")
    source.add_argument("--archive-dir", type=Path, help="directory holding exactly one archive")
    parser.add_argument("--version", required=True)
    args = parser.parse_args(argv)
    if args.archive_dir:
        archives = sorted(args.archive_dir.glob("symbrain_*"))
        if len(archives) != 1:
            raise SystemExit(f"expected one archive in {args.archive_dir}, found {archives}")
        extracted = Path(tempfile.mkdtemp(prefix="symbrain-release-extract-"))
        if archives[0].suffix == ".zip":
            with zipfile.ZipFile(archives[0]) as archive:
                archive.extractall(extracted)
        else:
            with tarfile.open(archives[0]) as archive:
                archive.extractall(extracted, filter="tar")
        return main(["--dir", str(extracted), "--version", args.version])
    candidate = args.dir.resolve()
    exe = ".exe" if os.name == "nt" else ""
    rust, go = candidate / ("symbrain" + exe), candidate / ("symbrain-go" + exe)
    for binary in [rust, go]:
        if not binary.is_file() or (os.name != "nt" and not os.access(binary, os.X_OK)):
            raise SystemExit(f"missing executable {binary.name}")

    with tempfile.TemporaryDirectory(prefix="symbrain-release-smoke-") as raw:
        scratch = Path(raw)
        env = environment(candidate, scratch)
        work = scratch / "work"
        work.mkdir()

        version = run(rust, ["version"], env, work)
        if version.returncode != 0 or args.version.encode() not in version.stdout:
            raise SystemExit(f"symbrain version did not report {args.version}: {version!r}")

        native, oracle = run(rust, FALLBACK_ARGS, env, work), run(go, FALLBACK_ARGS, env, work)
        if b"no Go fallback was found" in native.stderr:
            raise SystemExit("Rust CLI did not find the packaged symbrain-go on PATH")
        if native.returncode == 0 or (native.returncode, native.stdout, native.stderr) != (
                oracle.returncode, oracle.stdout, oracle.stderr):
            raise SystemExit(f"fallback output differs from the Go oracle:\nrust={native!r}\ngo={oracle!r}")
        if not oracle.stderr:
            raise SystemExit("Go oracle did not report the malformed flag")
    print(f"release smoke: PASS {args.version}: version reported; Rust reached the packaged symbrain-go and matched Go")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
