#!/usr/bin/env python3
"""Record a successful immediately preceding owned build from a clean checkout."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--kind", choices=("go", "rust"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.source.resolve(strict=True)
    repo = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], cwd=source, text=True).strip())
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=repo), "build source must be clean"
    ref = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    files = subprocess.check_output(["git", "ls-files", "browse"], cwd=repo, text=True).splitlines()
    if args.kind == "go":
        files = [name for name in files if name.endswith(".go") or name in ("browse/go.mod", "browse/go.sum")]
        sdk = subprocess.check_output(["go", "version", "-m", str(args.binary.resolve())], text=True)
    else:
        files = [name for name in files if name.endswith((".rs", ".toml")) or name == "browse/Cargo.lock"]
        sdk = subprocess.check_output(["rustc", "-Vv"], text=True)
    assert files, "no recorded build source"
    hashes = {}
    for name in files:
        data = (repo / name).read_bytes()
        assert data == subprocess.check_output(["git", "show", ref + ":" + name], cwd=repo)
        hashes[name] = hashlib.sha256(data).hexdigest()
    receipt = dict(source_revision=ref, source_clean=True, kind=args.kind,
        binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(), sdk=sdk, source_sha256=hashes,
        attestation="successful build immediately precedes this receipt in the owned runner; source bytes verified against immutable Git objects")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes((json.dumps(receipt, indent=2) + "\n").encode())


if __name__ == "__main__":
    main()
