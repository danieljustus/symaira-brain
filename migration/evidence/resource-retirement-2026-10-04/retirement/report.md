# Completed worktree retirement

Root approved exactly the15 listed in prioritized-15.json after full independent read-only audit. Ten were retired using normal `git worktree remove`, in the requested order. The loop stopped immediately when free space exceeded the2.5GiB budget. Five remain intact: doctor765-stdout, doctor765-output, doctor765-json, doctor765-windows and usage768-argv. No other worktree, branch, Git object, target, shared cache, port or product process was altered by this author.

Free space at start: 731.938MiB; at the budget stop: 2682.859MiB (2.620GiB). Observed available-space increase: 1950.922MiB (1.905GiB). This is not an exact>=2GiB reclaim claim. The final independent snapshot had 2386.086MiB globally available after two new concurrently registered source trees appeared (guard805-config-control and source765-windows-job-runtime-review). The task did not restart the remaining deletions after the original budget stop. Global use can continue changing; the scheduler should read actual free space before allocation.

| Removed worktree | HEAD | Published checkpoint | Observed delta MiB | Branch/tree |
|---|---|---|---:|---|
| symaira-memory649-root | ec57961f | 803 | 120.5 | preserved |
| symaira-skills794-wide-root | cec81cfc | 794 | 70.7 | preserved |
| symaira-memory803-deadline | a0ec18b8 | 803 | 105.9 | preserved |
| symaira-memory649-integrated | 63fbd320 | 803 | 115.6 | preserved |
| symaira-decisions | 6c6881db | 794 | 64.7 | preserved |
| symaira-doctor806-journal-import | 74dd7aea | 806 | 295.1 | preserved |
| symaira-doctor806-windows-job-diagnostic | e7541a33 | 806 | 295.2 | preserved |
| symaira-doctor806-source-progress-fix | 7ea3cfb9 | 806 | 294.6 | preserved |
| symaira-doctor806-source-progress | 9faee8b0 | 806 | 294.4 | preserved |
| symaira-doctor806-owned-paths | c6723ab2 | 806 | 294.2 | preserved |

Before each removal, rechecked exactHEAD and symbolic branch, persistent branchref, strict published-checkpoint ancestry, clean tracked/untracked status, every ignored path, absence of targets, all tracked source/proof SHA/length/blob/type/native permissions, visible exe/cwd/FD/environment consumers, and Docker's zero running-container result. Only verified CPython cache derivatives were removed beforehand; each original pyc is separately gzip/SHA/length retained. No --force or broad git clean was used. A second immediate status/HEAD/process check preceded normal removal. Every branch, commit tree, restoration command and complete tracked-file/native-mode manifest SHA was checked afterwards and remains accessible in the preserved symaira-brain Git common directory. All13 explicitly protected active paths and the five unremoved candidates still exist.

`journal.jsonl` is fsynced before/after commands and derivative removals, retaining exact argv, exit, raw stdout/stderr, free before/after and all verification/restoration facts. `completed.json`, `result.json`, `final-verification.json` and before/after worktree inventories retain complete outcomes. Two root-owned services—dockerd198/containerd249—remain UNVERIFIED for their privileged process references, as explicitly accepted by Root; no global-zero-user claim was made. Visible agent consumers were zero. No running Docker containers existed. Zombies were separately classified as exited records with no live address space/file table.

The first shell launch failed during redirection because the author-owned output directory had not yet been created. Python, candidate prechecks and deletions had not started. The exact launch failure is retained in initial-launch-failure.json; creating only the owned output directory corrected it. All subsequent candidate prechecks/removals returned zero. Any candidate/action failure would have stopped the retirement; no force or bypass path exists.

The long-term decision is prepared in adr-ready.md. Root can retain these complete workspace proofs in the chosen repository checkpoint. No GitHub operation occurred.
