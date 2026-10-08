# Independent corrected full-layer review: PR #797 / #621

Disposition: **approved for the scoped render-observation correction, subject to integration of corrected PR #794 and fresh exact-head native CI**. The original linked-unreadable finding is closed; no additional actionable defect was confirmed in this reviewed scope. The reviewer did not implement this correction.

- Reviewed immutable HEAD: `0efdc66f8f7786fe42eb9db062bfbc5ae817c807`.
- Previous reviewed HEAD: `ddd34beee51ef83bc0d115f3ad6b3038b7a970aa`.
- Current dependency/base: `d7d0acda1ec5d07c8cfb18f8b6ba67060a4d608c`.
- Main integration: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
- Frozen Go source: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`.
- Worktree: `/workspace/symaira-skills621`; clean before and after review. Candidate source, fixtures and GitHub state were not edited. Generated test outputs and reviewer receipts are separate from the candidate.

## Finding closure

The original reviewer demonstrated that a genuine managed link into a cache containing a nested link, FIFO or excessive depth returned stale installation diagnostics without any render fields. The corrected `compare_one` keeps its render-observation callback available on the comparison error path, once a fresh render has been successfully loaded/materialized. Comparison errors now produce the same stale status/error/summary through that callback; render inspection independently returns `unreadable` with its error. A failed safe scan does not expose partial cache hashes or change persisted marker modes. Unreadable links remain `symlink`; successful verification of the exact cache relationship permits presentation mode `linked`.

I independently reproduced genuine Go-created installs in five fresh isolated roots: clean, cache-only reference edit, nested outside link, FIFO and depth overflow. Each native JSON and table invocation had a five-second timeout, empty PATH and no Go fallback. Clean and edited caches retain their correct in-sync/drift observations. All three unsafe caches now retain `render_status=unreadable`, stale installation state, equal installation/render diagnostics, the unchanged stale summary and persisted symlink mode. No render-drift hashes or outside sentinel contents appear on the unsafe paths. Filesystem snapshots of directories, regular-file hashes, links and special-file types were identical before and after both invocations, including the library/SSOT, markers, installation, base and cache.

## Full-layer coverage

Read every changed production module and all corrective code/test changes, and reviewed the complete scoped implementation against the dependency base:

- CLI status fallback eligibility for explicit cached targets, dynamic config and OpenCode safety precedence; status options, JSON extension fields and conditional RENDER table output.
- Install/status metadata structures, lock acquisition, marker/ownership validation and existing source/error classification.
- `status_compare.rs` full success/error flow, fresh target materialization, three-way drift, original summary and mode behavior.
- New `render_status.rs` complete cache observation, deterministic union of changed paths/hashes, absent-cache shape, unreadable classification and verified linked presentation.
- Shared `drift.rs` depth/entry/per-file/aggregate hashing limits and no-follow/special-file rejection, plus existing capability, destination and shared-lock helpers.
- `sync.rs` explicitly disables the new observation, preserving stored modes and synchronization/conflict policy. No cache edit is copied back into the library.
- Added library and CLI tests, including copied-cache behavior and genuine linked unreadable trees, JSON/table reporting and filesystem snapshots.
- Workflow changes preserve the three native OS gates, actual immutable Go process execution and always/error/14-day evidence retention.
- Versioned migration/README/ADR/contract-matrix changes and evidence provenance. SKL-008 remains unique and distinct from preflight SKL-007.
- `scripts/skills-render-drift/run.sh`, README, `skills_render_contract.py`, its adversarial controls and the complete changed integration in `rust-differential.py`. Projection accepts only the six explicitly named old fixtures, validates new fields/hashes/mode/summary first and preserves actual baseline bytes; it does not hide arbitrary differences.

The original full review/failure observations are retained. The correction changes observational error handling, not install repair or migration ownership. Dynamic-config and unqualified/multi-target migration remains scoped to #764; the broader bounds work remains #476. No frozen Go production source/fixture, dependency, credential, permission or Brain/Guard boundary change is introduced.

## Independent checks actually executed

- Verified the exact HEAD and clean state, then rehashed all **11 relevant source inputs**: three corrected paths match the correction receipt and eight unchanged paths match the prior source-bound report.
- Verified actual binary identities: native Rust SHA-256 `1f7ab418d8702de6897eed69f4fe7aecff7dca9581e73308e510312b4922f5e4`, immutable Go SHA-256 `a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`.
- Independently ran `cargo test --locked --all-features -p symbrain-cli --test skills_render_drift -p symbrain-skills --test render_drift_status`, with worktree-owned output, umask 022, subreaper and required actual Go oracle: **10 parent tests passed, zero failures/ignored**. This includes **12 actual Go/native clean/edit reports across all six targets**, preserving existing report fields and summaries. Literal new observations: `/tmp/symaira-pr797-corrected-independent-live.json`; execution log: `/tmp/symaira-pr797-corrected-independent-tests.log`.
- Independently ran four Python projection-validator control tests: **4 passed**.
- Independently ran the five genuine-Go-install probes described above, JSON and table each, with unchanged before/after filesystem snapshots. Literal receipt: `/tmp/symaira-pr797-corrected-independent-probes.json`.
- Diff whitespace and final immutable HEAD/clean-state checks passed.

The author's larger primary suite and 504-case historical differential were not rerun wholesale or relabeled as reviewer executions. This review proves the Linux scoped correction, not native macOS/Windows execution. The branch still inherits old dependency D7; corrected #794 code and its actual squash main must be integrated before #797 is ready. Review relevant integrated differences and require fresh final-head native CI and normal branch protection before merging or closing #621. Approval of this scoped code does not approve its old dependency's separately corrected flag-byte defect or close #764/#476.
