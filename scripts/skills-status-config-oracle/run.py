#!/usr/bin/env python3
"""Go-vs-Rust proof and negative controls for the native `skills status`
environment-only config shape (`SYMSKILLS_LIBRARY_DIR` / `SYMSKILLS_BASE_DIR`
for `--target opencode`).

Usage: run.py GO_BINARY RUST_BINARY [--mutants MUTANT_TARGET_DIR]
              [--evidence-dir DIR] [--only NAME[,NAME...]]

1. Runs every `skills_status_config_*` case of scripts/rust-differential.py
   (this includes `skills_status_config_targets_*`: `skills targets` is
   config-independent in Go, so every env/config shape is native there):
   native cases run the Rust binary with every Go fallback route removed and
   must leave the fixture root untouched; `fallback` cases must reach the
   missing-fallback diagnostic when no Go route exists.
2. With `--mutants`, applies each single-line source mutation to
   rust/symbrain-cli/src/skills_cli.rs, builds it into MUTANT_TARGET_DIR and
   requires the same case set to FAIL on the expected cases. The source file
   is restored byte-for-byte afterwards, even on failure.

Every mutant is classified KILLED, SURVIVED (it ran to completion and the
expected cases did not all fail) or INCONCLUSIVE(timeout|build|harness). Only an
all-KILLED run exits 0; an inconclusive replay is never reported as SURVIVED and
never as KILLED. Each replay keeps its evidence under
EVIDENCE_DIR/<NN>-<name>/ (default MUTANT_TARGET_DIR/evidence/<stamp>-<pid>):
full stdout/stderr, result.json (case ids, mutant executable sha256), the
differential journal (distinct root per replay, with per-case timeout events and
raw child streams) and, unless KILLED, the mutant executable itself.

Set SYMBRAIN_MUTANT_CARGO (default `cargo`) to wrap the build, for example
`/Users/daniel/.local/bin/dev-external cargo`.
"""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "rust/symbrain-cli/src/skills_cli.rs"
# Mutants may patch either file; a pattern must match exactly one of them once.
SOURCES = (SOURCE, ROOT / "rust/symbrain-cli/src/skills_doctor_config.rs")
PREFIX = "skills_status_config_"

# chmod 000 does not restrict root, so these cases only exist for an
# unprivileged user; a mutant is then judged on its remaining expectations.
UNPRIVILEGED_ONLY = {
    "skills_status_config_fallback_unreadable_config_dir_json",
    "skills_status_config_fallback_library_mode_000_json",
    "skills_status_config_fallback_library_ancestor_search_only",
    "skills_status_config_fallback_library_ancestor_search_only_111",
    "skills_status_config_fallback_base_ancestor_search_only",
    "skills_status_config_fallback_base_ancestor_search_only_111",
    "skills_status_config_fallback_library_mode_444_json",
    "skills_status_config_doctor_config_dir_unreadable_resets_env_json",
}
PRIVILEGED = hasattr(os, "geteuid") and os.geteuid() == 0

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
        "    if config_files_need_go()\n        || std::env::var_os(\"SYMSKILLS_CACHE_DIR\")",
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
        "    if !path.is_absolute()\n        || !is_lexically_clean(text)",
        "    if !is_lexically_clean(text)",
        ["skills_status_config_fallback_relative_clean_library"],
    ),
    "accept-linked-library": (
        "            Ok(metadata) if metadata.is_dir() => {\n                if fs::read_dir(&prefix).is_err() {\n                    return false;\n                }\n            }\n",
        "            Ok(_) => {}\n",
        [
            "skills_status_config_fallback_library_symlink_json",
            "skills_status_config_fallback_library_symlinked_ancestor_json",
        ],
    ),
    "follow-symlinked-ancestor": (
        "        match fs::symlink_metadata(&prefix) {",
        "        match fs::metadata(&prefix) {",
        [
            "skills_status_config_fallback_library_symlink_json",
            "skills_status_config_fallback_library_symlinked_ancestor_json",
            "skills_status_config_fallback_base_symlinked_ancestor_json",
        ],
    ),
    "accept-unclean-path": (
        "        || !is_lexically_clean(text)\n",
        "",
        [
            "skills_status_config_fallback_library_dotdot_json",
            "skills_status_config_fallback_library_dot_segment",
            "skills_status_config_fallback_library_trailing_slash",
        ],
    ),
    "accept-unlistable-ancestor": (
        "            Ok(metadata) if metadata.is_dir() => {\n                if fs::read_dir(&prefix).is_err() {\n                    return false;\n                }\n            }\n",
        "            Ok(metadata) if metadata.is_dir() => {}\n",
        [
            "skills_status_config_fallback_library_ancestor_search_only",
            "skills_status_config_fallback_library_ancestor_search_only_111",
            "skills_status_config_fallback_base_ancestor_search_only",
            "skills_status_config_fallback_base_ancestor_search_only_111",
        ],
    ),
    "accept-unsearchable-leaf": (
        "    !present || fs::metadata(path.join(\".\")).is_ok()",
        "    true",
        ["skills_status_config_fallback_library_mode_444_json"],
    ),
    "is-file-config-probe": (
        "    matches!(fs::metadata(path), Err(error) if is_go_not_exist(&error))",
        "    !path.is_file()",
        [
            "skills_status_config_fallback_config_toml_is_directory_json",
            "skills_status_config_fallback_project_config_is_directory",
            "skills_status_config_fallback_config_dir_is_file_json",
            "skills_status_config_fallback_config_symlink_loop_json",
            "skills_status_config_fallback_unreadable_config_dir_json",
        ],
    ),
    "any-probe-error-is-absent": (
        "    matches!(fs::metadata(path), Err(error) if is_go_not_exist(&error))",
        "    fs::metadata(path).is_err()",
        [
            "skills_status_config_fallback_config_dir_is_file_json",
            "skills_status_config_fallback_config_symlink_loop_json",
            "skills_status_config_fallback_unreadable_config_dir_json",
        ],
    ),
    "missing-home-guard": (
        "            if !go_home_defined() {\n                // Go's",
        "            if false {\n                // Go's",
        [
            "skills_status_config_fallback_home_unset_json",
            "skills_status_config_fallback_home_empty",
            "skills_status_config_fallback_default_home_unset_json",
            # `skills status` without --target is also routed to Go by the
            # home check inside `has_dynamic_config` (mutant
            # dynamic-config-ignores-home), so that shape is covered by two
            # independent layers and no single-line mutant can fail it.
        ],
    ),
    "xdg-dot-segment-accepted": (
        "    if has_dot_segment(&text) || (cfg!(windows) && windows_path_needs_go(&text)) {",
        "    if cfg!(windows) && windows_path_needs_go(&text) {",
        ["skills_status_config_fallback_xdg_dotdot_through_symlink_json"],
    ),
    "xdg-relative-honoured": (
        "        .filter(|path| path.is_absolute())\n    {\n        Some(xdg) => xdg,",
        "    {\n        Some(xdg) => xdg,",
        [
            "skills_status_config_fallback_xdg_relative_home_config_file",
            "skills_status_config_fallback_xdg_empty_home_config_file",
        ],
    ),
    "doctor-gated-on-config": (
        "            parse_list_flags(&args[1..]).is_err()\n                || !go_home_defined()\n                || doctor_config::resolve().is_none()\n",
        "            args.len() != 1 || has_dynamic_config()\n",
        [
            "skills_status_config_doctor_env_library_dir_json",
            "skills_status_config_doctor_env_all_overrides_table",
            "skills_status_config_doctor_env_invalid_vcs_bool_resets_json",
            "skills_status_config_doctor_config_is_directory_resets_env_json",
            "skills_status_config_doctor_flag_target_ignored_json",
        ],
    ),
    "doctor-ignores-home": (
        "                || !go_home_defined()\n                || doctor_config::resolve().is_none()\n",
        "                || doctor_config::resolve().is_none()\n",
        [
            "skills_status_config_fallback_doctor_home_unset_abs_xdg_json",
            "skills_status_config_fallback_doctor_home_unset_abs_xdg",
            "skills_status_config_doctor_fallback_home_unset_with_env_json",
        ],
    ),
    "doctor-accepts-bad-flags": (
        "            parse_list_flags(&args[1..]).is_err()\n                || !go_home_defined()\n                || doctor_config::resolve().is_none()\n",
        "            !go_home_defined() || doctor_config::resolve().is_none()\n",
        [
            "skills_status_config_doctor_fallback_unknown_flag",
            "skills_status_config_doctor_fallback_unknown_flag_with_env_json",
            "skills_status_config_doctor_fallback_target_without_value",
            "skills_status_config_doctor_fallback_positional_argument",
        ],
    ),
    "doctor-invalid-bool-applies-env": (
        "        let Some(enabled) = go_bool(&value) else {\n            return Some(defaults);\n        };\n",
        "        let enabled = go_bool(&value).unwrap_or(true);\n",
        [
            "skills_status_config_doctor_env_invalid_vcs_bool_resets_json",
            "skills_status_config_doctor_env_invalid_vcs_bool_resets_table",
            "skills_status_config_doctor_env_empty_vcs_bool_resets_json",
        ],
    ),
    "doctor-directory-config-is-absent": (
        "        Ok(metadata) if metadata.is_dir() => Probe::Fails,\n",
        "        Ok(metadata) if metadata.is_dir() => Probe::Absent,\n",
        [
            "skills_status_config_doctor_config_is_directory_resets_env_json",
            "skills_status_config_doctor_config_is_directory_resets_env_table",
            "skills_status_config_doctor_project_config_is_directory_resets_env_json",
            "skills_status_config_doctor_project_config_is_directory_after_regular_global_json",
        ],
    ),
    "doctor-stat-error-is-absent": (
        "        Err(_) if cfg!(unix) => Probe::Fails,\n",
        "        Err(_) if cfg!(unix) => Probe::Absent,\n",
        [
            "skills_status_config_doctor_config_dir_is_file_resets_env_json",
            "skills_status_config_doctor_config_symlink_loop_resets_env_json",
            "skills_status_config_doctor_config_dir_unreadable_resets_env_json",
        ],
    ),
    "doctor-accepts-unclean-base": (
        "        && is_lexically_clean(text)\n        && json_plain(text)\n",
        "        && json_plain(text)\n",
        [
            "skills_status_config_doctor_fallback_home_unclean_doubled_slash_json",
            "skills_status_config_doctor_fallback_home_dot_segment_table",
            "skills_status_config_doctor_fallback_xdg_config_unclean_json",
            "skills_status_config_doctor_fallback_xdg_data_dotdot_json",
        ],
    ),
    "doctor-ignores-pwd": (
        "    if !cwd.to_str().is_some_and(json_plain) || !pwd_is_physical(&cwd) {\n",
        "    if !cwd.to_str().is_some_and(json_plain) {\n",
        [
            "skills_status_config_doctor_fallback_pwd_symlink_json",
            "skills_status_config_doctor_fallback_pwd_symlink_table",
        ],
    ),
    "doctor-accepts-line-separator": (
        "    !text.contains(['\\u{2028}', '\\u{2029}'])\n",
        "    true\n",
        [
            "skills_status_config_doctor_fallback_env_line_separator_json",
            "skills_status_config_doctor_fallback_env_paragraph_separator_table",
        ],
    ),
    "targets-gated-on-config": (
        "parse_scope_flag(&args[1..]).is_err() || !go_home_defined()",
        "parse_scope_flag(&args[1..]).is_err() || has_dynamic_config()",
        [
            "skills_status_config_targets_env_library_dir_json",
            "skills_status_config_targets_env_all_overrides_table",
            "skills_status_config_targets_env_invalid_vcs_bool_json",
            "skills_status_config_targets_config_custom_target_json",
            "skills_status_config_targets_config_malformed_json",
            "skills_status_config_targets_config_is_directory_json",
            "skills_status_config_targets_project_config_custom_target_project_scope_json",
        ],
    ),
    "targets-ignores-home": (
        "parse_scope_flag(&args[1..]).is_err() || !go_home_defined()",
        "parse_scope_flag(&args[1..]).is_err()",
        [
            "skills_status_config_fallback_targets_home_unset_abs_xdg_json",
            "skills_status_config_fallback_targets_home_unset_abs_xdg",
            "skills_status_config_targets_fallback_home_empty_with_env_json",
            "skills_status_config_targets_fallback_home_unset_with_config_json",
        ],
    ),
    "targets-accepts-bad-flags": (
        "parse_scope_flag(&args[1..]).is_err() || !go_home_defined()",
        "!go_home_defined()",
        [
            "skills_status_config_targets_fallback_unknown_scope_with_env",
            "skills_status_config_targets_fallback_unknown_flag_with_config",
        ],
    ),
    "home-gate-after-unsafe-marker": (
        "            if !go_home_defined() {\n                // Go's",
        "            if !go_home_defined() && opencode_status_needs_go(&scope).is_ok() {\n                // Go's",
        [
            "skills_status_config_fallback_home_unset_symlinked_project_root",
            "skills_status_config_fallback_home_unset_symlinked_project_root_json",
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


KILLED = "KILLED"
SURVIVED = "SURVIVED"
INCONCLUSIVE = "INCONCLUSIVE"
TIMEOUT_LINE = re.compile(r"^TIMEOUT case=(\S+) ", re.MULTILINE)


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def differential(go: Path, rust: Path, journal: Path | None = None) -> subprocess.CompletedProcess[str]:
    env = dict(os.environ, SYMBRAIN_DIFFERENTIAL_CASE_PREFIX=PREFIX)
    env.pop("SYMBRAIN_DIFFERENTIAL_JOURNAL", None)
    if journal is not None:
        # The journal refuses an existing root, so every replay needs its own.
        env["SYMBRAIN_DIFFERENTIAL_JOURNAL"] = str(journal)
    return subprocess.run(
        [sys.executable, str(ROOT / "scripts/rust-differential.py"), str(go), str(rust)],
        env=env, text=True, capture_output=True, check=False,
    )


def failed_cases(expect: list[str], stderr: str) -> list[str]:
    """Expected cases reported as failed, matched as whole `case: ` line starts."""
    lines = stderr.splitlines()
    return [case for case in expect if any(line.startswith(f"{case}: ") for line in lines)]


def timed_out_cases(stderr: str, journal: Path | None) -> list[str]:
    """Cases whose child hit the differential deadline (stderr line or journal event)."""
    found = list(dict.fromkeys(TIMEOUT_LINE.findall(stderr)))
    phases = journal / "phases.jsonl" if journal is not None else None
    if phases is not None and phases.is_file():
        for line in phases.read_text(encoding="utf-8").splitlines():
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if row.get("phase") == "child_timeout" and row.get("case") not in found:
                found.append(row["case"])
    return found


def classify(returncode: int, stderr: str, expect: list[str], timeouts: list[str]) -> tuple[str, str, list[str]]:
    """Return (status, reason, expected cases seen failing) for a finished replay."""
    failed = failed_cases(expect, stderr)
    if timeouts:
        return INCONCLUSIVE, "timeout", failed
    if returncode not in (0, 1) or "Traceback (most recent call last)" in stderr:
        return INCONCLUSIVE, "harness", failed
    if returncode != 0 and len(failed) == len(expect):
        return KILLED, "expected cases failed", failed
    return SURVIVED, "expected cases did not all fail", failed


def build_mutant(name: str, target_dir: Path, old: str, new: str, dest: Path) -> Path:
    """Build one mutant into `dest`; the source is restored even on failure."""
    matches = [(path, path.read_bytes()) for path in SOURCES if old in path.read_text()]
    assert len(matches) == 1, f"mutant {name}: pattern must match exactly one source"
    source, original = matches[0]
    text = original.decode()
    assert text.count(old) == 1, f"mutant {name}: pattern must match exactly once"
    try:
        source.write_text(text.replace(old, new, 1))
        env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
        subprocess.run(
            [*shlex.split(os.environ.get("SYMBRAIN_MUTANT_CARGO", "cargo")),
             "build", "-p", "symbrain-cli", "--locked"],
            cwd=ROOT, env=env, check=True, capture_output=True, text=True,
        )
    finally:
        source.write_bytes(original)
    kept = dest / f"symbrain-{name}"
    shutil.copy2(target_dir / "debug/symbrain", kept)
    return kept


def write_text(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8", errors="replace")


def replay(index: int, name: str, mutant: tuple[str, str, list[str]], expect: list[str],
           go: Path, target_dir: Path, evidence: Path | None) -> dict:
    """Build and replay one mutant; always return a classified record."""
    old, new, _ = mutant
    work = Path(tempfile.mkdtemp(prefix="status-mutant-"))
    folder = None
    if evidence is not None:
        folder = evidence / f"{index:02d}-{name}"
        folder.mkdir(parents=True, exist_ok=False)
    record: dict = {"mutant": name, "expected_cases": expect, "evidence": str(folder) if folder else None}
    binary = None
    try:
        try:
            binary = build_mutant(name, target_dir, old, new, work)
        except subprocess.CalledProcessError as error:
            record.update(status=INCONCLUSIVE, reason="build", returncode=error.returncode)
            if folder is not None:
                write_text(folder / "build.stdout", error.stdout or "")
                write_text(folder / "build.stderr", error.stderr or "")
            record["tail"] = ((error.stdout or "") + (error.stderr or ""))[-2000:]
            return record
        except (OSError, AssertionError) as error:
            record.update(status=INCONCLUSIVE, reason="harness", returncode=None, tail=repr(error))
            return record
        record["mutant_sha256"] = sha256_file(binary)
        journal = folder / "journal" if folder else None
        mutated = differential(go, binary, journal)
        timeouts = timed_out_cases(mutated.stderr, journal)
        status, reason, failed = classify(mutated.returncode, mutated.stderr, expect, timeouts)
        record.update(status=status, reason=reason, returncode=mutated.returncode,
                      expected_failures_seen=failed, timed_out_cases=timeouts,
                      tail=(mutated.stdout + mutated.stderr)[-2000:])
        if folder is not None:
            write_text(folder / "stdout.txt", mutated.stdout)
            write_text(folder / "stderr.txt", mutated.stderr)
            record["stdout_sha256"] = hashlib.sha256(mutated.stdout.encode(errors="replace")).hexdigest()
            record["stderr_sha256"] = hashlib.sha256(mutated.stderr.encode(errors="replace")).hexdigest()
            if status != KILLED:
                # A non-killed replay is the evidence; keep the exact executable.
                shutil.copy2(binary, folder / binary.name)
        return record
    except Exception as error:  # keep the remaining mutants running
        record.update(status=INCONCLUSIVE, reason="harness", returncode=None, tail=repr(error))
        return record
    finally:
        if folder is not None:
            write_text(folder / "result.json", json.dumps(record, indent=2, sort_keys=True) + "\n")
        shutil.rmtree(work, ignore_errors=True)


def pop_option(args: list[str], flag: str) -> str | None:
    if flag not in args:
        return None
    index = args.index(flag)
    value = args[index + 1]
    del args[index:index + 2]
    return value


def main() -> int:
    args = sys.argv[1:]
    mutants_opt = pop_option(args, "--mutants")
    mutant_dir = Path(mutants_opt).resolve() if mutants_opt else None
    evidence_opt = pop_option(args, "--evidence-dir")
    only_opt = pop_option(args, "--only")
    only = set(only_opt.split(",")) if only_opt else None
    if only is not None and not only <= set(MUTANTS):
        print(f"unknown mutants: {sorted(only - set(MUTANTS))}", file=sys.stderr)
        return 2
    if len(args) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    go, rust = (Path(arg).resolve() for arg in args)
    evidence = None
    if evidence_opt:
        evidence = Path(evidence_opt).resolve()
    elif mutant_dir is not None:
        evidence = mutant_dir / "evidence" / f"{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}-{os.getpid()}"
    if evidence is not None:
        evidence.mkdir(parents=True, exist_ok=False)
        print(f"EVIDENCE {evidence}")
    cases = load_cases()
    native = [case for case in cases if case.native]
    fallback = [case for case in cases if case.expect_fallback]
    assert native and fallback and len(native) + len(fallback) == len(cases)
    assert all(case.read_only for case in native)

    before = [hashlib.sha256(path.read_bytes()).hexdigest() for path in SOURCES]
    result = differential(go, rust, evidence / "baseline-journal" if evidence else None)
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    if evidence is not None:
        write_text(evidence / "baseline.stdout", result.stdout)
        write_text(evidence / "baseline.stderr", result.stderr)
    expected_line = f"{len(cases)} parity cases passed"
    if result.returncode != 0 or expected_line not in result.stdout:
        print(f"FAIL baseline: rc={result.returncode}, expected {expected_line!r}", file=sys.stderr)
        return 1
    print(f"BASELINE {len(cases)} cases ({len(native)} native, {len(fallback)} fallback) matched Go")

    if mutant_dir is None:
        return 0
    records = []
    for index, (name, mutant) in enumerate(MUTANTS.items(), start=1):
        if only is not None and name not in only:
            continue
        expect = mutant[2]
        if PRIVILEGED:
            expect = [case for case in expect if case not in UNPRIVILEGED_ONLY]
        if not expect:
            print(f"MUTANT {name}: SKIPPED (needs an unprivileged user)")
            continue
        record = replay(index, name, mutant, expect, go, mutant_dir, evidence)
        records.append(record)
        seen = len(record.get("expected_failures_seen", []))
        label = record["status"] if record["status"] != INCONCLUSIVE else f"{INCONCLUSIVE}({record['reason']})"
        print(f"MUTANT {name}: {label} (rc={record['returncode']}, "
              f"expected failures seen: {seen}/{len(expect)}"
              + (f", timed-out cases: {record['timed_out_cases']}" if record.get("timed_out_cases") else "")
              + (f", mutant sha256 {record['mutant_sha256']}" if record.get("mutant_sha256") else "")
              + ")")
        if record["status"] != KILLED:
            print(record.get("tail", ""), file=sys.stderr)
            if record.get("evidence"):
                print(f"  evidence retained: {record['evidence']}", file=sys.stderr)
    if [hashlib.sha256(path.read_bytes()).hexdigest() for path in SOURCES] != before:
        print("FAIL: a mutated source file was not restored", file=sys.stderr)
        return 1
    counts = {status: sum(1 for r in records if r["status"] == status)
              for status in (KILLED, SURVIVED, INCONCLUSIVE)}
    if evidence is not None:
        write_text(evidence / "summary.json", json.dumps(
            {"counts": counts, "mutants": records}, indent=2, sort_keys=True) + "\n")
    print(f"SUMMARY {counts[KILLED]} killed, {counts[SURVIVED]} survived, "
          f"{counts[INCONCLUSIVE]} inconclusive of {len(records)}")
    if counts[SURVIVED] or counts[INCONCLUSIVE]:
        bad = [f"{r['mutant']}={r['status']}" + (f"({r['reason']})" if r["status"] == INCONCLUSIVE else "")
               for r in records if r["status"] != KILLED]
        print(f"FAIL: mutants not killed: {bad}", file=sys.stderr)
        return 1
    print(f"{len(records)}/{len(records)} mutants killed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
