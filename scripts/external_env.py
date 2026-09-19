"""Re-exec direct local runners through the external-storage environment."""

from __future__ import annotations

from collections.abc import Mapping
import os
from pathlib import Path
import sys


EXTERNAL_ROOT = Path("/Volumes/1TB_NVMe_SN850X")
EXTERNAL_PATHS = (
    "TMPDIR",
    "TMP",
    "TEMP",
    "GOTMPDIR",
    "GOPATH",
    "GOTELEMETRYDIR",
    "GOCACHE",
    "GOMODCACHE",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "PYTHONPYCACHEPREFIX",
    "SYMAIRA_EXTERNAL_RUNTIME_ROOT",
    "GUARD_DECIDE_EVIDENCE",
    "GUARD_REPAIR_OUTPUT",
)


def paths_are_external(env: Mapping[str, str], root: Path) -> bool:
    """Report whether every local runner output path resolves below ``root``."""
    if env.get("SYMAIRA_EXTERNAL_ENV_READY") != "1":
        return False
    for name in EXTERNAL_PATHS:
        value = env.get(name)
        if not value:
            return False
        try:
            if not Path(value).expanduser().resolve(strict=False).is_relative_to(root):
                return False
        except OSError:
            return False
    return True


def external_environment_ready(env: Mapping[str, str] | None = None) -> bool:
    """Require the mounted NVMe and every generated-output path on macOS."""
    try:
        root = EXTERNAL_ROOT.resolve(strict=True)
    except OSError:
        return False
    return root.is_dir() and root.is_mount() and paths_are_external(os.environ if env is None else env, root)


def ensure_external_environment(script_file: str) -> None:
    """Keep local macOS runner temp files and caches on the attached NVMe."""
    if sys.platform != "darwin" or os.environ.get("CI"):
        return
    if external_environment_ready():
        return

    script = Path(script_file).resolve()
    wrapper = next(
        (
            candidate
            for parent in script.parents
            for candidate in (parent / "run-external-env.sh", parent / "scripts" / "run-external-env.sh")
            if candidate.is_file()
        ),
        None,
    )
    if wrapper is None:
        raise RuntimeError("scripts/run-external-env.sh is required for local macOS runners")
    env = os.environ.copy()
    env["SYMAIRA_EXTERNAL_ENV_READY"] = "1"
    os.execve(
        "/bin/bash",
        ["/bin/bash", str(wrapper), sys.executable, str(script), *sys.argv[1:]],
        env,
    )
