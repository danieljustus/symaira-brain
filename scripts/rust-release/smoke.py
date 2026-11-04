#!/usr/bin/env python3
"""Smoke-test one extracted release archive on its native platform.

  smoke.py --dir extracted --version V
  smoke.py --archive-dir dist --version V   # extracts the single archive in dist

Checks that the archive holds no Go fallback, that `symbrain version` reports
V, that `symbrain help` succeeds and that a malformed flag is rejected by the
native CLI itself. The environment is minimal and owned: an empty HOME/XDG
tree and a PATH of the extracted archive plus the system directories.
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

MALFORMED_ARGS = ["skills", "targets", "--candidate-smoke-unknown"]


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
    rust = candidate / ("symbrain" + exe)
    if not rust.is_file() or (os.name != "nt" and not os.access(rust, os.X_OK)):
        raise SystemExit(f"missing executable {rust.name}")
    leftovers = sorted(p.name for p in candidate.iterdir() if p.name.startswith("symbrain-go"))
    if leftovers:
        raise SystemExit(f"archive still ships a Go fallback: {leftovers}")

    with tempfile.TemporaryDirectory(prefix="symbrain-release-smoke-") as raw:
        scratch = Path(raw)
        env = environment(candidate, scratch)
        work = scratch / "work"
        work.mkdir()

        version = run(rust, ["version"], env, work)
        if version.returncode != 0 or args.version.encode() not in version.stdout:
            raise SystemExit(f"symbrain version did not report {args.version}: {version!r}")
        usage = run(rust, ["help"], env, work)
        if usage.returncode != 0 or b"Usage:" not in usage.stdout:
            raise SystemExit(f"symbrain help failed: {usage!r}")
        malformed = run(rust, MALFORMED_ARGS, env, work)
        if malformed.returncode == 0 or not malformed.stderr or b"Go fallback" in malformed.stderr:
            raise SystemExit(f"native CLI did not reject the malformed flag itself: {malformed!r}")
    print(f"release smoke: PASS {args.version}: Rust-only archive; version, help and flag validation are native")
    return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
