# Retire reconstructible idle source worktrees

Retain commits, branch refs, full source and raw evidence in Git. Reclaim an obsolete checkout only after its clean HEAD is a strict ancestor of a published candidate, every tracked byte/blob/native mode matches its retained manifest, no target/cache/unknown untracked runtime exists, and fresh visible process and Docker observations show no consumer. Preserve restoration argv and native modes. Use normal git worktree remove, never force removal or ref/object deletion. Keep active and exact latest candidates.

The first authorized run retired ten of fifteen checked source worktrees and stopped at2.620GiB available space; its measured gain was1.905GiB. A later separately authorized run retired three of the remaining five and stopped at2.513GiB, gaining782258176 bytes. Doctor Windows and Usage argv remained. Concurrent owned Go-cache cleanup completed before the second baseline and is excluded from its gain. These are measured available-space changes, not an assumption based on du.

The two privileged dockerd/containerd services were not inspectable through process files and remain explicitly UNVERIFIED. Docker reported zero running containers and visible agent consumer checks passed; there is no global zero-consumer claim. Root accepted that bounded visibility before the routine reversible retirement.

Full journals, original failures, tracked-file SHA/blob/native-mode manifests, restoration commands, original and final observations, compressed derivatives and all raw bindings are retained in migration/evidence/resource-retirement-2026-10-04/full-audit.tar.gz. Every archive member round-trips against archive-manifest.json. No compiler, target or product state was retired.
