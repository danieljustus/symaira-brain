#!/usr/bin/env python3
"""Require Apple's Accepted status and a stable submission ID; print only that ID."""

import argparse
import json
from pathlib import Path
import uuid


def accepted_id(path: Path, expected: str | None = None) -> str:
    value = json.loads(path.read_text())
    if not isinstance(value, dict) or value.get("status") != "Accepted":
        raise ValueError("notarization was not Accepted")
    identifier = value.get("id")
    if not isinstance(identifier, str) or str(uuid.UUID(identifier)) != identifier.lower():
        raise ValueError("notarization ID is missing or invalid")
    if expected is not None and identifier != expected:
        raise ValueError("notarization read-back identifies a different submission")
    return identifier


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("receipt", type=Path)
    parser.add_argument("--expected-id")
    args = parser.parse_args()
    try:
        print(accepted_id(args.receipt, args.expected_id))
    except (OSError, ValueError, TypeError) as error:
        parser.exit(1, f"notarization verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
