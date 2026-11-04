"""Owned native fixture executables for the key-provider harnesses, without Go.

The programs replace the former ``*.go.in`` sources that were compiled with
``go build`` at test time. They are single-file, std-only Rust sources built
with plain ``rustc`` from the browse toolchain, so the harnesses still observe
real native processes: the process-identity checks compare the kernel's
executable path, which an interpreted script would not satisfy.
"""
from __future__ import annotations

import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCES = {
    "provider": HERE / "state_key_fixture.rs",
    "owner-control": HERE / "state_key_control.rs",
    "path-control": HERE / "startup_key_path_control.rs",
}


def source(kind: str) -> Path:
    return SOURCES[kind]


def install(kind: str, destination: Path) -> Path:
    """Compile the ``kind`` program to ``destination`` as an owner-only executable."""
    if os.name == "nt":
        raise RuntimeError("fixture programs are Unix-only; Windows is not a supported target")
    # cwd=HERE selects the pinned browse toolchain through rust-toolchain.toml.
    subprocess.run(
        ["rustc", "--edition", "2021", "-C", "opt-level=1", "-C", "debuginfo=0",
         "-o", str(destination), str(SOURCES[kind])],
        cwd=HERE, check=True, capture_output=True, timeout=300,
    )
    destination.chmod(0o700)
    return destination
