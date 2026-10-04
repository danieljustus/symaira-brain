# Portable export

The Git bundle contains all refs visible to git bundle --all, including every local branch and historical commit reachable from those refs. It does not contain untracked files, reflog-only unreachable objects, worktree indexes, installed SDKs or live runtime trees.

The proof archive is curated and PARTIAL. Every regular file in each selected_complete_directories entry is included, plus selected raw CI logs and the partial Root Skills reader. Its manifest explicitly lists omitted top-level proof entries. References from included proofs to other cloud files can remain unresolved: this archive is not a transitively complete SDK/native-binary/restoration archive.

Do not run archived helper scripts automatically. Read their exact source, review status and runtime requirements first. The Brain method has REQUEST_CHANGES.

Verify downloads using the published SHA256SUMS.txt, then inspect archive members before extraction into a new directory. On macOS: shasum -a 256 FILE. Restore proofs with tar -xzf symaira-macbook-proofs-20261004.tar.gz -C NEW_DIRECTORY.

The archive retains relative paths review-proof/... and docs/...; original absolute /workspace references are historical provenance. Do not rewrite them as verified Mac metadata.

For optional offline source recovery: git bundle verify symaira-macbook-source-20261004.bundle in a repository. git clone -b handoff/20261004/coordination symaira-macbook-source-20261004.bundle symaira-brain-recovered. To recover any particular branch explicitly use git fetch BUNDLE refs/heads/BRANCH:refs/heads/RECOVERED_BRANCH. The bundle has no credentials; inspect and set origin to the authorized GitHub remote when resuming.
