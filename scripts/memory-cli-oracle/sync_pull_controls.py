#!/usr/bin/env python3
"""Source-mutant controls for the data-carrying pulls of the `sync_relay` family (Unix only).

The real-process mutants of ``sync_relay_controls.py`` perturb one observable
field after the fact. These controls instead change the *native source* in the
way a faulty port would and rebuild the CLI from a scratch copy of the tree:

* ``conflict``: an equal timestamp overwrites the local memory (``<=`` -> ``<``);
* ``tombstone``: an equal-aged tombstone no longer blocks a pulled memory;
* ``delete``: a remote delete removes a memory that is strictly newer (``>`` -> ``>=`` flipped);
* ``pagination``: only the first page is fetched (``next_cursor`` ignored);
* ``cursor``: the pagination cursor is never sent, every page repeats the first request;
* ``clock``: the stored cursor ignores the timestamps of pulled entries;
* ``audit``: the audit row of a pulled insert is not written;
* ``pending``: the push stream is not probed after a pull;
* ``rollback``: a memory whose statement fails is skipped and the rest of the batch is committed.

Each mutant is replayed against the scenarios that exercise it (``SYNC_RELAY_ONLY``)
and must be rejected by the family; the scenarios it breaks must all be ones
the mutation can explain. A mutant that survives, or breaks nothing it should,
fails the control.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

# (name, file, old, new, scenarios replayed, scenarios that may break)
MUTANTS = [
    ("conflict", "rust/symbrain-memory/src/sync_apply.rs",
     "if memory.updated_at <= stored_time(&existing)? {", "if memory.updated_at < stored_time(&existing)? {",
     r"^(lww-|pull-same-id)", r"equal|same-id|second-run|-a$|^lww-"),
    ("tombstone", "rust/symbrain-memory/src/sync_apply.rs",
     'if op == "delete" && memory.updated_at <= ts {', 'if op == "delete" && memory.updated_at < ts {',
     r"^tombstone-", r"^tombstone-"),
    ("delete", "rust/symbrain-memory/src/sync_apply.rs",
     "if stored_time(&local)? > delete.deleted_at {", "if stored_time(&local)? >= delete.deleted_at {",
     r"^(delete-|mixed-)", r"^(delete-|mixed-)"),
    ("pagination", "rust/symbrain-cli/src/memory_cli/sync_pull.rs",
     "Some(cursor) => next = Some(cursor),", "Some(_) => return Some(Pull::Pages(pages)),",
     r"^(pages-|both-pages|both-failure|mixed-delete-then|mixed-memory-delete)", r"^(pages-|both-|mixed-)"),
    ("cursor", "rust/symbrain-cli/src/memory_cli/sync_pull.rs",
     "let url = page_url(source.base, since, next.as_deref());", "let url = page_url(source.base, since, None);",
     r"^(pages-two|pages-ten|pages-cursor-0)", r"^pages-"),
    ("clock", "rust/symbrain-cli/src/memory_cli/sync_decode.rs",
     ".map(|memory| memory.updated_at)\n            .chain(self.deleted.iter()", ".map(|_| zero_time())\n            .chain(self.deleted.iter()",
     r"^clock-", r"^clock-"),
    ("audit", "rust/symbrain-memory/src/sync_apply.rs",
     '            "set",\n', '            "mutant",\n',
     r"^(pull-new-memory|pull-two|pull-id-0)", r"^pull-"),
    ("pending", "rust/symbrain-memory/src/sync_apply.rs",
     "&& pending_changes(&transaction, since).ok()?", "&& pending_changes(&transaction, since).ok()? && false",
     r"^handoff-both-", r"^handoff-both-"),
    ("rollback", "rust/symbrain-memory/src/sync_apply.rs",
     "if upsert_if_newer(&transaction, memory)? {", "if upsert_if_newer(&transaction, memory).unwrap_or(false) {",
     r"^handoff-failing-statement", r"^handoff-"),
]


def run(command, **kwargs):
    return subprocess.run(command, capture_output=True, **kwargs)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--go", type=Path)
    parser.add_argument("--cargo", default="cargo", help="cargo command prefix, e.g. 'dev-external cargo'")
    parser.add_argument("--work", type=Path, help="scratch directory for the mutated copy of the tree")
    parser.add_argument("--only", help="comma separated mutant names")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if os.name != "posix":
        print(json.dumps({"sync_pull_controls_skipped": "posix only"}))
        return
    repo = Path(__file__).resolve().parents[2]
    cargo = shlex.split(args.cargo)
    selected = set(args.only.split(",")) if args.only else None
    results = []
    with tempfile.TemporaryDirectory(prefix="memory-cli-pull-control-", dir=args.work) as temporary:
        work = Path(temporary) / "tree"
        work.mkdir()
        shutil.copytree(repo / "rust", work / "rust", ignore=shutil.ignore_patterns("target"))
        for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"):
            if (repo / name).exists():
                shutil.copy2(repo / name, work / name)
        metadata = run(cargo + ["metadata", "--format-version", "1", "--no-deps", "--locked"], cwd=work, check=True)
        binary = Path(json.loads(metadata.stdout)["target_directory"]) / "debug" / "symbrain"
        for name, relative, old, new, replay_only, may_break in MUTANTS:
            if selected and name not in selected:
                continue
            path = work / relative
            original = path.read_text()
            assert original.count(old) == 1, (name, "mutation site is not unique", original.count(old))
            path.write_text(original.replace(old, new))
            try:
                build = run(cargo + ["build", "-p", "symbrain-cli", "--locked"], cwd=work)
                assert build.returncode == 0, (name, "mutant does not build", build.stderr.decode()[-2000:])
                report = Path(temporary) / f"{name}.json"
                command = [sys.executable, str(repo / "scripts/memory-cli-oracle/replay.py"), "--rust", str(binary),
                           "--family", "sync_relay", "--report", str(report)]
                if args.go:
                    command += ["--go", str(args.go.resolve())]
                child = run(command, cwd=repo, env=dict(os.environ, SYNC_RELAY_ONLY=replay_only))
            finally:
                path.write_text(original)
            receipt = json.loads(report.read_bytes())
            assert child.returncode != 0, ("mutant survived", name)
            assert receipt["cases"] >= 1, (name, "mutant replayed too few scenarios", receipt["cases"])
            rejected = [record["name"] for record in receipt["records"] if not record["match"]]
            assert rejected, (name, "nothing rejected")
            stray = [item for item in rejected if not re.search(may_break, item)]
            assert not stray, (name, "unexplained rejections", stray)
            results.append(dict(mutant=name, replayed=receipt["cases"], rejected=len(rejected),
                                first_rejected=rejected[:3]))
    if args.report:
        args.report.write_text(json.dumps(dict(controls=results), indent=2) + "\n")
    print(json.dumps({"sync_pull_source_mutants_rejected": len(results), "detail": results}))


if __name__ == "__main__":
    main()
