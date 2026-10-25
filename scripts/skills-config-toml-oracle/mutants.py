#!/usr/bin/env python3
"""Negative controls for the Go-exact symskills config TOML decoder (SKL-018).

Each mutant rewrites one line of rust/symbrain-skills/src/config_toml.rs, runs
the fixture replay (`cargo test -p symbrain-skills --test config_toml_oracle`)
and must make it FAIL. The source is restored byte for byte afterwards, even on
failure or interrupt.

Usage: mutants.py [--only NAME[,NAME...]]

Set SYMBRAIN_MUTANT_CARGO (default `cargo`) to wrap the build, for example
`/Users/daniel/.local/bin/dev-external cargo`. Exit 0 only when every mutant is
KILLED; a mutant that builds and passes is SURVIVED, one that does not build
or time out is INCONCLUSIVE (never counted as killed).
"""
from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "rust/symbrain-skills/src/config_toml.rs"

# (name, exact original line fragment, replacement)
MUTANTS = [
    ("go-type-int", '"int64"', '"int"'),
    ("go-type-time", '"time.Time"', '"time"'),
    ("parse-bool-drop-T", '"1" | "t" | "T" | "TRUE"', '"1" | "t" | "TRUE"'),
    ("bom-not-stripped", "text.strip_prefix('\\u{feff}')", "text.strip_prefix('\\u{fffe}')"),
    ("second-bom-accepted", "if text.starts_with('\\u{feff}') {", "if false {"),
    ("leap-second-accepted", "second > 59", "second > 60"),
    ("multiline-inline-table-accepted", "source.contains(['\\n', '\\r'])", "false"),
    ("vcs-inline-table-ignored", 'root.get("vcs").and_then(Item::as_table_like)', 'root.get("vcs").and_then(Item::as_table)'),
    ("global-label-swapped", '[("global", global), ("project", project)]', '[("project", global), ("global", project)]'),
    ("kelvin-fold-dropped", "'\\u{212a}' => 'k',", "'\\u{212a}' => '\\u{212a}',"),
    ("long-s-fold-dropped", "'\\u{17f}' => 's',", "'\\u{17f}' => '\\u{17f}',"),
    ("field-collision-ignored", "&& slots[index].replace(item).is_some()", "&& { slots[index] = Some(item); false }"),
    ("empty-targets-override", "if !typed.targets.is_empty() {", "if true {"),
    ("capabilities-not-merged", ".extend(capabilities);", ".extend(BTreeMap::new());"),
    ("non-table-map-errors", "        return Ok(None);", "        return Err(NeedsGo::TypeMismatch);"),
    ("quote-widened", "(0x20..0x7f).contains(&byte)", "(0x20..=0xff).contains(&byte)"),
]


def run(cargo: list[str]) -> tuple[str, str]:
    try:
        proc = subprocess.run(
            [*cargo, "test", "-p", "symbrain-skills", "--locked", "--test", "config_toml_oracle"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=1500,
        )
    except subprocess.TimeoutExpired:
        return "INCONCLUSIVE(timeout)", ""
    out = proc.stdout + proc.stderr
    if proc.returncode == 0:
        return "SURVIVED", out
    if "error[E" in out or "could not compile" in out:
        return "INCONCLUSIVE(build)", out
    if "test result: FAILED" in out:
        return "KILLED", out
    return "INCONCLUSIVE(harness)", out


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--only", default="")
    args = parser.parse_args()
    only = {name for name in args.only.split(",") if name}
    cargo = shlex.split(os.environ.get("SYMBRAIN_MUTANT_CARGO", "cargo"))
    original = SOURCE.read_bytes()
    text = original.decode()
    results = {}
    try:
        for name, old, new in MUTANTS:
            if only and name not in only:
                continue
            if text.count(old) != 1:
                results[name] = f"INCONCLUSIVE(pattern matched {text.count(old)}x)"
            else:
                SOURCE.write_text(text.replace(old, new, 1))
                results[name], _ = run(cargo)
            SOURCE.write_bytes(original)
            print(f"{results[name]:28} {name}", flush=True)
    finally:
        SOURCE.write_bytes(original)
    bad = {name: state for name, state in results.items() if state != "KILLED"}
    print(f"{len(results) - len(bad)}/{len(results)} killed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
