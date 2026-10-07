# Independent review: usage credential routing increment (#768)

Reviewer: read-only independent Codex agent `review_pr800`.
Base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
Reviewed head: `b10a699abbb1a006cfcbb161d3ad1b255fa93e93`.
Worktree: `/workspace/symaira-usage768`.
Disposition: **no actionable production findings; this scoped increment is suitable for acceptance after exact-head native Linux/macOS/Windows CI and protected checks pass**. Issue #768 must remain open. This review does not certify the remaining file/JWT/override/automatic-Keychain paths or complete CLI cutover.

## Scope and source reconstruction

Read root `AGENTS.md`, all changed production code, all ten `provider_config` include fragments, changed provider tests, the dedicated credential-reference integration test, workflow, five runner/generator/comparator/receipt/control scripts and their README, ADR, migration ledger/plan and tracked source-bound receipt. Read the immutable Go usage credential adapter and memory secrets wrapper, plus pinned CoreKit v0.17.0 reference parsing, vault and unsupported-platform Keychain implementations. The reviewed immutable Go oracle is `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`; the author's clean Linux proof binds source head `effa08bc6a42a72726e1281490cbb877a5600512`, whose recorded source hashes were checked against the reviewed candidate.

Reassembled the ten fragments in include order and compared the resulting module with E8's original 2,053-line `provider_config.rs`. Saved the independently reconstructed semantic diff in `/tmp/symaira-usage768-independent-reconstructed-source.diff`. Apart from organization, whitespace and comments, production behavior changes are confined to replacing the usage-only environment resolver with the existing shared `resolve_reference` adapter, deleting the redundant command helper and removing environment-reference-only fallback signals. Constructors, credential-file decoding, subprocess runner, shared reference resolver and Keychain discovery logic retain their prior implementation. Every production fragment is below 400 lines; `include!` preserves the existing module's private visibility and state ownership.

## Correctness, boundaries and maintainability

The adapter preserves the `resolve ENV_NAME:` wrapper around the shared error chain, deprecated `vault://` alias, source tagging, command argument separator, deadline, empty-secret behavior and non-macOS Keychain rejection. In particular, installing a `security` executable on Linux/Windows does not make an explicit `keychain://` reference supported. Successful provider constructors use the resolved credential in their request headers; failures preserve the full report's auth status/detail/source instead of collapsing resolver diagnostics. The Go failure source value `vault` is retained even for an environment or Keychain reference failure, as required by the actual Go adapter.

Only the proved environment-reference fallback reasons are removed. Credential files, ambiguous JWT expiry, base/workspace overrides, mismatched Windows HOME/USERPROFILE and automatic Claude host-Keychain discovery still trigger their conservative Go gates. The CLI comment and ADR accurately describe this partial migration. Neither a complete credential migration nor closure of #768 is claimed.

The shared resolver's bounded-output and UTF-8 policy predates this change and is explicitly documented in the ADR: malformed UTF-8 and excessive child output are rejected rather than silently changing authentication bytes. The fresh corpus proves valid synthetic UTF-8 credentials and enumerated errors; arbitrary malformed authentication bytes remain outside its parity claim. The shared runner still uses bounded per-stream output, private temporary files, a five-second deadline, direct argv execution and existing child cleanup. No new unsafe code, master-key access, secret store, external endpoint or operator-state mutation is introduced. Removing the duplicate resolver reduces the chance that administration and usage interpret the same reference differently.

The harness isolates HOME/USERPROFILE/XDG roots and PATH, uses owned synthetic `symvault`/`security` executables and supplies canned HTTP 401 responses via injected Go/native transports. It suppresses automatic host Claude discovery and restores private mutation fixtures. Actual CLI cases use credential errors, owned roots and a nonexistent Go fallback executable. No live provider account, operator credential file or real system Keychain entry is needed or represented as a passing host-integration check.

## Independent executable verification

Executed the complete focused runner independently at clean reviewed head B10 using the session subreaper, `set -euo pipefail`, `umask 022`, owned target directory, Rust 1.98.0 and Go 1.26.7. Fresh receipt: `/tmp/symaira-usage768-independent-process-report.json`; logs and underlying JSON reports: `/tmp/symaira-usage768-independent-process-report.evidence`; combined log: `/tmp/symaira-usage768-independent-gate.log`.

* All **44** real immutable-Go constructor/full-report/request-header comparisons passed, with 44 distinct required case IDs and zero failures. The dedicated integration test actually executed with `--ignored`: one passed, zero failed/ignored. Its strict corpus check does not accept an absent, truncated or duplicate fixture.
* All **110** actual native-versus-Go CLI cases matched stdout bytes, stderr bytes and exit status, with 110 distinct required IDs and zero failures. Native SHA256: `65b32db3080e2c651777fa430727425419cd136f0192e0149597d7827d832db1`; Go SHA256: `0654376cf8fc33c3be0b03bd394b665842d04e6659b3dfd67c0d6786d2f136e2`.
* All **five** actual mutation controls rejected their intended fault: altered expected CLI exit (exit 1, byte/exit mismatch), missing CLI case (exit 1, missing case), removed Go fixture (exit 101, missing Go evidence), changed report (exit 101, full-report mismatch) and duplicate constructor case (exit 101, corpus mismatch). These controls execute the same comparator/integration processes rather than merely inspecting mutant data.
* The independently generated receipt binds B10 with `candidate_dirty: false`. All **41** recorded candidate source SHA256 values were independently recomputed and matched. Fresh oracle SHA256: `806537d86dd21433584ff2e02b2f6010158678ad1cfd35655a06f72ce5379987`.
* Inspected the author's broader clean-source validation receipt: 356 passed, zero failed, one ordinarily ignored fresh-oracle integration test, 31 test summaries; the dedicated gate then executes that integration. Strict Clippy, repository formatting, explicit fragment formatting and actionlint passed. Those broader tests/static checks were inspected, not claimed as independently rerun.

The native three-OS workflow uses repository-pinned SDKs, creates Windows executable fixtures with `.exe`, requires the complete nonzero corpus and all controls, and uploads the report plus available stage evidence even on failure. Missing evidence fails the upload step. Native macOS and Windows exact-head results remain a merge condition; this independent Linux run cannot replace them. Real host Keychain ACL/discovery integration remains explicitly outside this reference-adapter gate.

## Documentation nit and final disposition

The runner README at B10 says “four actual failing replay controls”; implementation, ADR and both receipts contain five. This is a nonblocking documentation count typo reported to the author for a docs-only correction. No production change or retesting is needed for that correction. An evidence-only or this README-only successor can carry this review after confirming the reviewed production/test/script hashes remain identical.

I made no repository source edit, commit, push, PR state change or merge. No remaining production correctness, security, resource-use or platform issue was found in the scoped increment. Keep #768 open for the documented remaining credential and routing contracts; accept this increment only after fresh native exact-head CI and protected checks.
