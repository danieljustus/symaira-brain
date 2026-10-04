#!/usr/bin/env python3
"""Prepare an owned probe module; no build or mutation of the frozen Go tree."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--frozen", type=Path, required=True)
    parser.add_argument("--owned-module", type=Path, required=True)
    args = parser.parse_args()
    frozen, owned = args.frozen.resolve(), args.owned_module.resolve()
    if owned.exists() or owned == frozen or frozen in owned.parents:
        raise SystemExit("owned probe module must be new and outside the frozen tree")
    source = Path(__file__).resolve().parents[2]
    retention = json.loads((source / "migration/evidence/brain-config13/initial-58fe/retention.json").read_text())
    expected = {entry["original_path"]: entry["sha256"] for entry in retention["entries"]}
    sources = ["go.mod", "go.sum", "internal/config/config.go"]
    for relative in sources:
        original = frozen / relative
        if expected.get(str(original)) != sha(original):
            raise SystemExit("frozen Go contract differs from retained census: " + relative)
    copied = {}
    for relative in sources:
        original, target = frozen / relative, owned / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(original, target)
        copied[relative] = dict(source=str(original), sha256=sha(target))
    target = owned / "cmd/config13-probe/main.go"
    target.parent.mkdir(parents=True)
    shutil.copyfile(Path(__file__).with_name("go_probe.go"), target)
    copied[str(target.relative_to(owned))] = dict(source=str(Path(__file__).with_name("go_probe.go")), sha256=sha(target))
    (owned / "probe-retention.json").write_text(json.dumps(dict(
        frozen_contract_unchanged=True, built=False, files=copied,
        intended_build="GOPROXY=off GOSUMDB=off go build -o OWNED_BINARY ./cmd/config13-probe",
    ), indent=2) + "\n")


if __name__ == "__main__":
    main()
