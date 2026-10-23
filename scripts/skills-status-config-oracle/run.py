#!/usr/bin/env python3
"""Go-vs-Rust proof and negative controls for the native `skills status`
environment-only config shape (`SYMSKILLS_LIBRARY_DIR` / `SYMSKILLS_BASE_DIR`
for `--target opencode`).

Usage: run.py GO_BINARY RUST_BINARY [--mutants MUTANT_TARGET_DIR]

1. Runs every `skills_status_config_*` case of scripts/rust-differential.py:
   native cases run the Rust binary with every Go fallback route removed and
   must leave the fixture root untouched; `fallback` cases must reach the
   missing-fallback diagnostic when no Go route exists.
2. With `--mutants`, applies each single-line source mutation to
   rust/symbrain-cli/src/skills_cli.rs, builds it into MUTANT_TARGET_DIR and
   requires the same case set to FAIL on the expected cases. The source file
   is restored byte-for-byte afterwards, even on failure.

Set SYMBRAIN_MUTANT_CARGO (default `cargo`) to wrap the build, for example
`/Users/daniel/.local/bin/dev-external cargo`.
"""
from __future__ import annotations

import hashlib
import importlib.util
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "rust/symbrain-cli/src/skills_cli.rs"
PREFIX = "skills_status_config_"

# name -> (old, new, case names that must fail)
MUTANTS = {
    "ignore-library-env": (
        "library_dir = config.library_dir.unwrap_or(library_dir);",
        "let _ = &config.library_dir;",
        ["skills_status_config_env_empty_library_orphaned_json"],
    ),
    "ignore-base-env": (
        "base_dir = config.base_dir.unwrap_or(base_dir);",
        "let _ = &config.base_dir;",
        ["skills_status_config_env_base_only_harness_changed_json"],
    ),
    "render-extension-on": (
        "    (library_dir, base_dir, home_dir, None)\n",
        "    (\n        library_dir,\n        base_dir,\n        home_dir,\n        Some(skills_data_root().join(\"rendered\")),\n    )\n",
        ["skills_status_config_env_symlink_insync_json"],
    ),
    "widen-to-other-targets": (
        "&& !(target.as_deref() == Some(\"opencode\") && status_env_config().is_some())",
        "&& !status_env_config().is_some()",
        [
            "skills_status_config_fallback_other_target",
            "skills_status_config_fallback_no_target",
        ],
    ),
    "accept-cache-dir": (
        "        || std::env::var_os(\"SYMSKILLS_CACHE_DIR\").is_some()\n",
        "",
        ["skills_status_config_fallback_cache_dir_json"],
    ),
    "accept-config-file": (
        "    if has_config_file()\n        || std::env::var_os(\"SYMSKILLS_CACHE_DIR\")",
        "    if std::env::var_os(\"SYMSKILLS_CACHE_DIR\")",
        [
            "skills_status_config_fallback_config_file_json",
            "skills_status_config_fallback_project_config_file",
        ],
    ),
    "accept-invalid-bool": (
        "&& !go_parse_bool(&value)",
        "&& false",
        [
            "skills_status_config_fallback_invalid_vcs_bool_json",
            "skills_status_config_fallback_empty_vcs_bool",
        ],
    ),
    "accept-relative-path": (
        "    if !path.is_absolute() {\n        return Err(GoOnlyConfig);\n    }\n",
        "",
        [
            "skills_status_config_fallback_relative_library_json",
            "skills_status_config_fallback_empty_library_value",
        ],
    ),
    "accept-linked-library": (
        "Ok(metadata) if metadata.is_dir() => Ok(Some(path)),",
        "Ok(_) => Ok(Some(path)),",
        [
            "skills_status_config_fallback_library_symlink_json",
            "skills_status_config_fallback_library_is_file",
        ],
    ),
    "write-a-file": (
        "    let (library_dir, base_dir, home_dir, render_dir) = status_dirs();\n",
        "    let (library_dir, base_dir, home_dir, render_dir) = status_dirs();\n"
        "    let _ = fs::create_dir_all(skills_data_root().join(\"mutant-scratch\"));\n",
        ["skills_status_config_env_symlink_insync_json"],
    ),
}


def load_cases():
    sys.path.insert(0, str(ROOT / "scripts"))
    spec = importlib.util.spec_from_file_location(
        "rust_differential", ROOT / "scripts/rust-differential.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return [case for case in module.CASES if case.name.startswith(PREFIX)]


def differential(go: Path, rust: Path) -> subprocess.CompletedProcess[str]:
    env = dict(os.environ, SYMBRAIN_DIFFERENTIAL_CASE_PREFIX=PREFIX)
    return subprocess.run(
        [sys.executable, str(ROOT / "scripts/rust-differential.py"), str(go), str(rust)],
        env=env, text=True, capture_output=True, check=False,
    )


def build_mutant(name: str, target_dir: Path, old: str, new: str) -> Path:
    original = SOURCE.read_bytes()
    text = original.decode()
    assert text.count(old) == 1, f"mutant {name}: pattern must match exactly once"
    try:
        SOURCE.write_text(text.replace(old, new, 1))
        env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
        subprocess.run(
            [*shlex.split(os.environ.get("SYMBRAIN_MUTANT_CARGO", "cargo")),
             "build", "-p", "symbrain-cli", "--locked"],
            cwd=ROOT, env=env, check=True, capture_output=True, text=True,
        )
    finally:
        SOURCE.write_bytes(original)
    kept = Path(tempfile.mkdtemp(prefix="status-mutant-")) / f"symbrain-{name}"
    shutil.copy2(target_dir / "debug/symbrain", kept)
    return kept


def main() -> int:
    args = sys.argv[1:]
    mutant_dir = None
    if "--mutants" in args:
        index = args.index("--mutants")
        mutant_dir = Path(args[index + 1]).resolve()
        del args[index:index + 2]
    if len(args) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    go, rust = (Path(arg).resolve() for arg in args)
    cases = load_cases()
    native = [case for case in cases if case.native]
    fallback = [case for case in cases if case.expect_fallback]
    assert native and fallback and len(native) + len(fallback) == len(cases)
    assert all(case.read_only for case in native)

    before = hashlib.sha256(SOURCE.read_bytes()).hexdigest()
    result = differential(go, rust)
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    expected_line = f"{len(cases)} parity cases passed"
    if result.returncode != 0 or expected_line not in result.stdout:
        print(f"FAIL baseline: rc={result.returncode}, expected {expected_line!r}", file=sys.stderr)
        return 1
    print(f"BASELINE {len(cases)} cases ({len(native)} native, {len(fallback)} fallback) matched Go")

    if mutant_dir is None:
        return 0
    survivors = []
    for name, (old, new, expect) in MUTANTS.items():
        binary = build_mutant(name, mutant_dir, old, new)
        mutated = differential(go, binary)
        output = mutated.stdout + mutated.stderr
        failed = [case for case in expect if f"{case}:" in mutated.stderr]
        # A killed mutant fails the run and names every expected case.
        killed = mutated.returncode != 0 and len(failed) == len(expect)
        print(f"MUTANT {name}: {'KILLED' if killed else 'SURVIVED'} "
              f"(rc={mutated.returncode}, expected failures seen: {len(failed)}/{len(expect)})")
        if not killed:
            survivors.append(name)
            print(output[-2000:], file=sys.stderr)
        shutil.rmtree(binary.parent, ignore_errors=True)
    if hashlib.sha256(SOURCE.read_bytes()).hexdigest() != before:
        print("FAIL: skills_cli.rs was not restored", file=sys.stderr)
        return 1
    if survivors:
        print(f"FAIL: surviving mutants: {survivors}", file=sys.stderr)
        return 1
    print(f"{len(MUTANTS)}/{len(MUTANTS)} mutants killed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
