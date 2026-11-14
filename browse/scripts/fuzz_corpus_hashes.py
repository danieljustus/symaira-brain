#!/usr/bin/env python3
"""Pin the tracked fuzz seed corpus to SHA-256 digests.

`--check` (default) fails when a seed is added, removed or changed relative to
`fuzz/corpus/hashes.json`; `--write` regenerates the manifest after a reviewed
seed change. libFuzzer must only ever run against a scratch copy of the corpus
(see `fuzz_smoke.sh`); this check proves the tracked seeds were not mutated.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

FUZZ_DIR = Path(__file__).resolve().parents[1] / "fuzz"
CORPUS = FUZZ_DIR / "corpus"
MANIFEST = CORPUS / "hashes.json"


def current() -> dict[str, dict[str, str]]:
    result: dict[str, dict[str, str]] = {}
    for target in sorted(p for p in CORPUS.iterdir() if p.is_dir()):
        result[target.name] = {
            seed.name: hashlib.sha256(seed.read_bytes()).hexdigest()
            for seed in sorted(target.iterdir())
            if seed.is_file()
        }
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="verify the manifest (default)")
    mode.add_argument("--write", action="store_true", help="regenerate the manifest")
    args = parser.parse_args()
    actual = current()
    if args.write:
        MANIFEST.write_text(json.dumps(actual, indent=2, sort_keys=True) + "\n")
        print(f"wrote {MANIFEST.relative_to(FUZZ_DIR.parent)}: "
              f"{len(actual)} targets, {sum(map(len, actual.values()))} seeds")
        return 0
    expected = json.loads(MANIFEST.read_text())
    problems = []
    for target in sorted(set(expected) | set(actual)):
        want, have = expected.get(target, {}), actual.get(target, {})
        for seed in sorted(set(want) | set(have)):
            if seed not in have:
                problems.append(f"{target}/{seed}: missing")
            elif seed not in want:
                problems.append(f"{target}/{seed}: not in manifest")
            elif want[seed] != have[seed]:
                problems.append(f"{target}/{seed}: digest changed")
    if problems:
        print("fuzz corpus drift:\n  " + "\n  ".join(problems), file=sys.stderr)
        return 1
    print(f"fuzz corpus ok: {len(actual)} targets, {sum(map(len, actual.values()))} seeds")
    return 0


if __name__ == "__main__":
    sys.exit(main())
