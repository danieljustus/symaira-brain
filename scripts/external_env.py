"""Re-exec direct local runners through the external-storage environment."""

from __future__ import annotations

import os
from pathlib import Path
import sys


def ensure_external_environment(script_file: str) -> None:
    """Keep local macOS runner temp files and caches on the attached NVMe."""
    if sys.platform != "darwin" or os.environ.get("CI"):
        return
    if os.environ.get("SYMAIRA_EXTERNAL_ENV_READY") == "1":
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
