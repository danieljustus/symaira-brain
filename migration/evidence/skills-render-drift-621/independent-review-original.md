# Independent full-layer review: PR #797 / issue #621

Disposition: **changes requested** for one confirmed P2 correctness defect. This is an independent read-only code review, not a merge approval or a substitute for integrated exact-head three-OS CI.

## Immutable revisions

- Repository: danieljustus/symaira-brain
- Reviewed HEAD: `ddd34beee51ef83bc0d115f3ad6b3038b7a970aa`
- Dependency/base: PR #794 `d7d0acda1ec5d07c8cfb18f8b6ba67060a4d608c`
- Main integration: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`
- Frozen production Go oracle: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`
- Worktree: `/workspace/symaira-skills621`; clean at review start and conclusion.

## Confirmed finding

**[P2] Retain the render report when linked-cache comparison fails.**

`rust/symbrain-skills/src/install/status_compare.rs:76` installs the new `render_status::inspect` callback, but `file_hashes(installed, false)?` at line 95 can return before the callback executes. For an ordinary managed symlink pointing at the render cache, a nested symlink, FIFO/special file, or excessive directory depth inside that cache triggers this path. `rust/symbrain-skills/src/install/status.rs:308` handles that error with the original outer `common` callback, whose render fields are all unset. Therefore the primary linked-render installation has no `render_status`, no `render_error`, and no RENDER table column exactly when its retained render is unreadable. This contradicts the documented unreadable-cache contract and makes linked installations observably less informative than copied installations.

Actual native CLI reproduction used genuine managed install/marker/base/cache artifacts created by the immutable Go `sync opencode`. Go was removed from the native environment, PATH pointed at an absent empty-bin directory, and each invocation had a five-second timeout. Clean cache reports `render_status=in-sync`, mode linked. Adding a FIFO, adding a nested symlink, and creating 34 nested directories each produces exit 0, stale status plus the existing diagnostic, mode symlink, and **no new render fields**. Literal four-case process results: `/tmp/symaira-pr797-independent-probes.json`. No production input, fixture or source was changed.

Recommended correction: construct the observational render report independently of successful three-way classification once a fresh target render exists, including the error path. Preserve existing stale/error/summary behavior and ensure an unsafe linked render is explicitly unreadable, without hashes from outside paths or any repair/SSOT writes. Add regressions for true cache-linked installs with nested links, special files and depth rejection; current unsafe-cache tests only exercise copy installations. Only present linked mode when the chosen verification policy actually establishes the exact cache relationship.

## Review coverage and conclusions

Inspected every changed file below, root AGENTS.md and docs/handoffs/end-work-cloud.md, and related status/comparison, marker, destination, cap_root, lock, replacement/materialization and CLI eligibility flows.

- Existing report semantics, marker modes and sync policy remain separate from the new observational fields. Sync explicitly disables render inspection. No cache-to-library copying or implicit repair was introduced.
- Ordinary rendered target transforms are applied before hash comparison; reference-file edits, additions and deletions are sorted deterministically. Empty hashes represent absence. Ordinary managed exact-cache symlinks use the linked presentation mode without rewriting persisted markers.
- Cache reads reuse no-follow directory capabilities, existing-only shared locks and existing per-file/aggregate/entry limits; this patch adds a depth limit. Existing special files and symlinks are refused. No demonstrated new external read/write or secret-disclosure defect was found. The error-path finding above still needs correction.
- JSON extension keys are optional and omitted for absent caches. Human rows append RENDER only when a render state exists. All six registered targets have clean/edit process coverage; project/config/unqualified migration coverage remains explicitly scoped to #764 rather than asserted complete.
- SKL-008 is present once and remains distinct from SKL-007 preflight. The legacy comparator accepts only six explicitly listed fixture names, validates the added fields, exact cache/base hashes, old summary and fields, and verified linked mode before reconstructing legacy bytes. It does not blindly erase differences. Native real-process coverage requires a Go oracle when configured and exactly 12 target/state combinations.
- Workflow adds a real Go build and actual native CLI execution to Linux, macOS and Windows jobs. Evidence artifact upload remains always/error/14 days. No Go production source or frozen fixtures changed.
- Source-bound Linux receipt is tied to clean tested source `93f023539fae2e2b9caa1b44e0b2e626dfd98cba`, parent PR794 and main E8. All 11 recorded candidate source hashes match the reviewed HEAD. Its 416-pass primary suite and 504-case differential are inspected historical execution evidence, not independently rerun wholesale in this review. Old comparator failures and negative controls remain preserved.

## Independent verification actually performed

- Verified immutable worktree HEAD, changed-file inventory, clean state and diff whitespace (exit 0).
- Rehashed all 11 source paths in the Linux receipt: 11/11 match.
- Four Python validator-control tests: four pass.
- Existing built Linux native render_drift_status executable: seven tests pass; existing built skills_render_drift CLI executable: one test passes. These were executed directly without rebuilding or sharing Cargo mutations; they do not imply native macOS/Windows verification.
- Four additional actual CLI probes described above, preserving the failed cases in a separate reviewer receipt.

## Acceptance still outstanding

PR794 must pass its own independent review and land first. PR797 must then integrate the actual squash main, fix the P2 finding, obtain review of the corrected immutable revision, and pass fresh native Linux/macOS/Windows acceptance and normal branch protection. Current queued or previous-head jobs are not substituted for those gates. No PR/issue state, source, branch, evidence in the worktree, or GitHub review was modified by this reviewer.

## Changed files inspected

- `.github/workflows/ci.yml`
- `MIGRATION-RUST.md`
- `README.md`
- `migration/contract-matrix.csv`
- `migration/evidence/skills-render-drift-621/legacy-comparator-failure.json`
- `migration/evidence/skills-render-drift-621/linux-failure-controls.json`
- `migration/evidence/skills-render-drift-621/linux-process-report.json`
- `rust/symbrain-cli/src/skills_cli.rs`
- `rust/symbrain-cli/tests/skills_render_drift.rs`
- `rust/symbrain-skills/src/install/drift.rs`
- `rust/symbrain-skills/src/install/mod.rs`
- `rust/symbrain-skills/src/install/render_status.rs`
- `rust/symbrain-skills/src/install/status.rs`
- `rust/symbrain-skills/src/install/status_compare.rs`
- `rust/symbrain-skills/src/install/sync.rs`
- `rust/symbrain-skills/tests/render_drift_status.rs`
- `scripts/rust-differential.py`
- `scripts/skills-render-drift/README.md`
- `scripts/skills-render-drift/run.sh`
- `scripts/skills_render_contract.py`
- `scripts/test_skills_render_contract.py`

## Reviewer executable and receipt hashes

- `/workspace/symaira-skills621/target/debug/symbrain`: SHA-256 `9c02023a5c6eff35e0e9f3ff865569702e13b2eac729d8c711c0a0d22508ef78`
- `/workspace/oracles/symbrain-go-dcddcef0`: SHA-256 `a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`
- `/tmp/symaira-pr797-independent-probes.json`: SHA-256 `ef4e03179dcee198108912a3f00ae9a47984168df0f53893cb86e038642d6a98`
