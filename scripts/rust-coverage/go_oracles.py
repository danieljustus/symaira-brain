#!/usr/bin/env python3
"""Add real Go oracle execution to coverage while retaining every unit-suite block."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ORACLES = ("policy", "catalog", "audit", "patterns-activity")


def read_profile(path):
    lines = path.read_text().splitlines()
    if not lines or lines[0] not in ("mode: set", "mode: count", "mode: atomic"):
        raise ValueError("invalid Go coverage mode")
    blocks = {}
    for line in lines[1:]:
        key, statements, hits = line.rsplit(" ", 2)
        value = (int(statements), int(hits))
        if key in blocks or any(number < 0 for number in value):
            raise ValueError("duplicate or invalid Go coverage block")
        blocks[key] = value
    if not blocks or sum(value[0] for value in blocks.values()) == 0:
        raise ValueError("empty Go coverage profile")
    return lines[0], blocks


def merge_profiles(mode, baseline, additional):
    merged = dict(baseline)
    for key, (statements, hits) in additional.items():
        if key not in baseline or statements != baseline[key][0]:
            raise ValueError(f"oracle changed the unit-suite denominator: {key}")
        total_hits = baseline[key][1] + hits
        merged[key] = (statements, int(total_hits > 0) if mode == "mode: set" else total_hits)
    return merged


def counts(blocks, product_only=False):
    values = [value for key, value in blocks.items()
              if not product_only or "/scripts/" not in key]
    total = sum(value[0] for value in values)
    covered = sum(statements for statements, hits in values if hits)
    return dict(covered=covered, count=total, percent=100 * covered / total if total else 0)


def profile_summary(path):
    _, blocks = read_profile(path)
    packages = {}
    for key, (statements, hits) in blocks.items():
        package = key.rsplit(":", 1)[0].rsplit("/", 1)[0]
        item = packages.setdefault(package, {"covered": 0, "count": 0})
        item["count"] += statements
        if hits:
            item["covered"] += statements
    return dict(total=round(counts(blocks)["percent"], 1), statements=counts(blocks),
                product_code_diagnostic=counts(blocks, product_only=True),
                packages={key: round(100 * value["covered"] / value["count"], 1)
                          if value["count"] else 0 for key, value in sorted(packages.items())})


def instrument(baseline_path, output, receipt):
    mode, baseline = read_profile(baseline_path)
    toolchain = "go" + next(line.split()[1] for line in (ROOT / "go.mod").read_text().splitlines()
                            if line.startswith("go "))
    env = dict(os.environ, GOTOOLCHAIN=toolchain)
    # Preserve only build-cache locations before replacing the runtime HOME.
    caches = subprocess.check_output(["go", "env", "GOCACHE", "GOMODCACHE", "GOPATH"],
                                     env=env, cwd=ROOT, text=True).splitlines()
    env.update(zip(("GOCACHE", "GOMODCACHE", "GOPATH"), caches))
    with tempfile.TemporaryDirectory(prefix="go-oracle-coverage-") as directory:
        root = Path(directory)
        for name in ("home", "config", "data", "cache", "profiles"):
            (root / name).mkdir()
        env.update(HOME=str(root / "home"), USERPROFILE=str(root / "home"),
                   XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"),
                   XDG_CACHE_HOME=str(root / "cache"), GOCOVERDIR=str(root / "profiles"))
        for oracle in ORACLES:
            subprocess.run(["go", "run", "-cover", "-covermode=" + mode.split()[1],
                            "-coverpkg=./...", "./scripts/" + oracle + "-oracle", "-check"],
                           cwd=ROOT, env=env, check=True)
        raw = root / "oracles.out"
        subprocess.run(["go", "tool", "covdata", "textfmt", "-i=" + str(root / "profiles"),
                        "-o=" + str(raw)], cwd=ROOT, env=env, check=True)
        oracle_mode, additional = read_profile(raw)
        if oracle_mode != mode:
            raise ValueError("unit and oracle coverage modes differ")
        merged = merge_profiles(mode, baseline, additional)
        output.write_text(mode + "\n" + "".join(
            f"{key} {statements} {hits}\n" for key, (statements, hits) in sorted(merged.items())))
        result = dict(oracles=list(ORACLES), toolchain=toolchain,
                      denominator_blocks=len(baseline), unit=counts(baseline), combined=counts(merged),
                      product_code_diagnostic=counts(merged, product_only=True),
                      baseline_sha256=hashlib.sha256(baseline_path.read_bytes()).hexdigest(),
                      oracle_profile_sha256=hashlib.sha256(raw.read_bytes()).hexdigest(),
                      combined_sha256=hashlib.sha256(output.read_bytes()).hexdigest(),
                      source_sha256={str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                                     for oracle in ORACLES for path in
                                     sorted((ROOT / "scripts" / (oracle + "-oracle")).glob("*.go"))})
        receipt.write_text(json.dumps(result, indent=2) + "\n")
        print(f"Go combined statements: {result['combined']['covered']}/{result['combined']['count']} "
              f"= {result['combined']['percent']:.2f}% (full unit denominator retained)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--summary-only", type=Path)
    args = parser.parse_args()
    if args.summary_only:
        result = profile_summary(args.summary_only)
        result.update(schema_version=1, commit_sha=subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip())
        args.out.write_text(json.dumps(result, indent=2) + "\n")
    elif args.baseline and args.receipt:
        instrument(args.baseline, args.out, args.receipt)
    else:
        parser.error("--baseline and --receipt, or --summary-only, required")


if __name__ == "__main__":
    main()
