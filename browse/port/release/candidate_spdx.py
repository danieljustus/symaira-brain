"""SPDX identity checks for merged unsigned Browse candidates."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

import verify


def verify_candidate_spdx(path: Path, archive: Path, implementation: str, version: str) -> None:
    verify._verify_spdx(path)
    try:
        document: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise verify.GateError(f"invalid candidate evidence {path}: {error}") from error
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    expected_name = f"{archive.name}.sbom"
    if (
        not isinstance(document, dict)
        or document.get("dataLicense") != "CC0-1.0"
        or document.get("SPDXID") != "SPDXRef-DOCUMENT"
        or document.get("name") != f"symbrowse-{implementation}-{archive.name}"
        or document.get("documentNamespace")
        != f"https://spdx.symaira.dev/symbrowse/{implementation}/{archive.name}#sha256-{digest}"
        or document.get("creationInfo")
        != {"created": "1970-01-01T00:00:00Z", "creators": ["Tool: symaira-browse dual release builder"]}
    ):
        raise verify.GateError(f"SPDX document identity mismatch for {expected_name}")
    packages = document.get("packages")
    if (
        not isinstance(packages, list)
        or len(packages) != 1
        or packages[0]
        != {
            "SPDXID": "SPDXRef-Package-symbrowse",
            "name": "symbrowse",
            "versionInfo": version.removeprefix("v"),
            "downloadLocation": "NOASSERTION",
            "filesAnalyzed": False,
            "checksums": [{"algorithm": "SHA256", "checksumValue": digest}],
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": "Apache-2.0",
            "copyrightText": "NOASSERTION",
        }
    ):
        raise verify.GateError(f"SPDX package identity or archive digest mismatch for {expected_name}")
