# Independent full-layer review: corrected native Memory CLI #758

Reviewer: independent read-only agent `review_pr800`; I did not author the Memory CLI changes.

Base/main: `31de72294521701fc4b1a0bce39f03cc34d72e7e`.
Reviewed clean publication head: `d8c90632763c7b8e805b2f1e2d287ce238fef52c`.
Tested clean source head: `2572f1bf28ac57c3af348245f1444aa6172ce997`.
Worktree: `/workspace/symaira-memory758-cli-fixes`.

**Disposition: no remaining actionable findings. The bounded increment is suitable for publication and acceptance after fresh exact-head native Linux/macOS/Windows CI and protected checks pass. This does not approve full #758 cutover or closure of release-dependent #649.**

Main is a real ancestor of the reviewed head. Source-to-publication changes comprise only the ADR and five evidence files: no production, test, harness or workflow changes. All 148 candidate files in the process manifest match the tested source and current reviewed head byte-for-byte. This is the actual replay manifest size, rather than the original rejected candidate's 142-file count.

## Original findings are corrected

**Default/legacy data resolution:** inspected the complete corrected memory-local resolver and frozen Go `internal/paths/paths.go`. Relative XDG_DATA_HOME is ignored; current and legacy directories share the same selected absolute-XDG-or-HOME root; directory tests determine preference. A regular file at the current namespace does not suppress an existing legacy directory. The general Core resolver remains untouched. Seven fresh actual-process path cases passed. I additionally reconstructed both original reviewer failures independently with separately seeded competing databases and harmless configuration. The Go stdout/stderr/exit bytes exactly reproduce the original failure receipts; corrected Rust now matches them. Every table, row and blob remained unchanged.

**Populated rules JSON escaping:** the corrected rules renderer applies the existing Go HTML/JavaScript-separator escaping to the entire rendered array, covering content, metadata keys/values and actors. The 32 read shapes now contain three real rules with Unicode, `<`, `>`, `&`, U+2028 and U+2029. I independently reconstructed the original single-rule failure. Its actual Go bytes exactly reproduce the original receipt, and corrected Rust matches them. All persisted tables/rows/blobs remained unchanged. The original rejected review, literal failure receipts, baseline binary identities and unsubreaped Doctor test failure remain tracked; they were not rewritten as successful evidence.

## Full inspected scope

Read applicable root `AGENTS.md`; reviewed all changed CLI production modules, their tests and interaction with root raw flag normalization: dispatch, native/fallback eligibility, raw OS paths, Go integer scanning and error precedence, help/usage grammar, kind aliases, configuration cache, complete 86-field schema, numeric conversions, global/project/environment merging, list/query-log/rules/search output, database selection and governed writes. The split gives focused modules of at most 298 lines; every changed production module is below the repository's 400-line limit.

Read the complete CLI replay, argument/configuration generators, path cases, Go schema reflection program, actual executable mutation wrapper, controls and README; both native three-OS workflows; both Memory ADRs; migration narrative, implementation plan and contract-matrix changes. Trace the local resolver and JSON fix against actual frozen Go source and process behavior. Review correctness, security boundaries, resource behavior, maintainability and native-platform assumptions. No unrelated domain routing, profile authority, secret exposure or fallback removal is introduced.

The entire changed `rust/symbrain-memory` production/test tree is byte-identical to `8d2eefc5c02a77de7eccddb486f5b227e91b079d`, which I previously reviewed independently in `/tmp/symaira-memory758-independent-review.md`. That earlier full review inspected the pinned Go EvidenceKit implementation, exact/normalized/fuzzy UTF-8 byte offsets and tie-breaking, strict validation, Go JSONL hashes, source/evidence persistence and transaction-owned reparenting, schema repair of all five #649 columns despite applied migration names, failure atomicity and concurrency. Its independently reproduced deferred-transaction regression was corrected with IMMEDIATE schema transactions and independently verified with real simultaneous public opens/writes. I checked the current migration/store/evidence interaction again and verified the complete crate diff is empty; I do not represent those earlier 42 independent Memory tests or 333 author consumer tests as newly rerun in this review.

The workflows require actual native Ubuntu/macOS/Windows processes, pinned Go 1.26.7 and pinned repository Rust, source/SDK receipts, replay and executable controls. Windows excludes Unix raw-byte argv cases, giving 553 rather than 590 comparisons; output and fixture mutation writes use explicit UTF-8 bytes. Native three-OS execution remains pending and is not replaced by this Linux verification.

## Fresh independent verification

* **590/590 actual Go/native Memory CLI comparisons:** 246 arguments, 302 configuration, 32 populated read shapes, three owned embedding HTTP requests and seven database-path cases. Go reflection executed and established all 86 configuration fields. The replay compares literal stdout/stderr/exit bytes, checks complete semantic database state and runs without an available Go fallback or operator HOME. Report: `/tmp/symaira-memory758-cli-corrected-independent-process.json`.
* **Two actual executable negative controls:** stdout semantic mutation and nonzero success exit each reject all 32 read cases with the intended replay assertion; database state stays unchanged. Report: `/tmp/symaira-memory758-cli-corrected-independent-controls.json`.
* **18 additional independently reconstructed probes:** all 15 original supplemental configuration cases plus both original path failures and the original populated-rules failure pass. Original actual Go outputs are byte-identical to the rejected receipts, confirming that the reproductions test the same behaviors. Report: `/tmp/symaira-memory758-cli-corrected-independent-extra.json`.
* **265/265 fresh actual Go/native Activity validation comparisons** using the corrected CLI, including integer overflow/underscore/raw-byte precedence. Its nine source hashes and both executable identities match the author's clean source-2572 evidence. Report: `/tmp/symaira-memory758-cli-corrected-independent-activity-process.json`.
* **119 fresh native Rust tests**, zero failures/ignored: all 117 CLI lib tests plus the Activity oracle and validation integration binaries. Binaries were independently SHA-verified against the clean source-bound receipt before execution. Logs: `/tmp/symaira-memory758-cli-corrected-independent-cli-lib.log`, `/tmp/symaira-memory758-cli-corrected-independent-activity-oracle.log`, `/tmp/symaira-memory758-cli-corrected-independent-activity-validation.log`.
* Fresh `cargo fmt --all -- --check` and actionlint for both Memory native workflows pass. The author's current exact-source strict Clippy/build logs and their literal hashes were inspected and verified; no duplicate Cargo build was necessary. The initial unchanged-source Doctor lifecycle failure is retained and the full reaped lib log correctly reports 117 passes.

Actual immutable Go oracle: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, Go SDK 1.26.7, `/workspace/oracles/symbrain-go-dcddcef0`, SHA-256 `a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`. Corrected native CLI SHA-256 `71091ff9a963d5effdcb0f741085ff7c6f4723e83a0cd0a8b5a7d7f78c2d170e`. All 12 archived Go CLI/memory source hashes match the authored immutable manifest. ConfigKit SHA-256 `bd812ed747352c76b9f223a19ec64970b61fda59050f7ed436f5b2e74b997218`; executed reflected schema SHA-256 `64d6052099e3d6147e478a667711c46576b71dfb644fe31c48e9a26620b9f62a`. Canonical 148-file candidate source manifest SHA-256 `afbf7b643717c8ea4675e8f8ae67f6f8c2797ae12b77c1156ade7c2b90944b1a`.

## Acceptance limits

The documented partial ownership is truthful. Governed/dynamic writes, configured Hamming prefilter, every database-open/error shape, new-file permissions, JSONL decoding, sync and serve remain Go-owned or pending. #758 remains open. #649 retains shipped repaired-store and Doctor diagnostic requirements. No native three-OS result or full cutover is claimed.

All subprocess checks used the session subreaper where relevant, `set -euo pipefail`, `umask 022` and disposable private roots. I made no repository source edit, commit, push, PR state change or merge. The accompanying source-bound JSON receipt records reviewed file hashes and fresh report identities.
