#!/usr/bin/env python3
"""Run native symbrowse identity checks before candidate packaging."""
from __future__ import annotations

import json
import subprocess
from pathlib import Path


def check_binary(binary: Path, version: str) -> dict[str, object]:
    """Require a native CLI to return its exact versionkit handshake."""
    result = subprocess.run(
        [str(binary), "version", "--json"], check=True, capture_output=True, text=True, timeout=30
    )
    try:
        identity = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ValueError(f"{binary} emitted invalid version JSON: {error}") from error
    if (
        not isinstance(identity, dict)
        or identity.get("tool") != "symbrowse"
        or identity.get("version") != version
        or not isinstance(identity.get("schema_version"), int)
        or identity["schema_version"] < 1
    ):
        raise ValueError(f"{binary} version identity mismatch: {identity!r}")
    return identity
