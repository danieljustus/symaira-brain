# Independent full-layer review: Usage Hermes file/JWT migration #768

Disposition: **REQUEST CHANGES**. Full changed-layer inspection is complete. One independently reproduced P2 regression remains; no other actionable findings were found. This disposition is specific to this immutable candidate and does not retire the remaining Usage migration scope.

## Immutable inputs and source binding

- Worktree: `/workspace/symaira-usage768-jwt`, clean at review completion; reviewer did not edit candidate, frozen Go, fixtures, workflow or evidence, and did not push, publish, merge or access operator credentials.
- Parent base: `117e4f37f6ce1c669e9856bcb90e41d8611913ab`.
- Production/harness/docs source commit: `63e2527c5bbec48643495d4a169bbdd3d50b6a47`.
- Final evidence-only candidate HEAD: `273a93c03d0cc526d81d962ac137a5a6fb0d73e9`.
- Frozen actual Go oracle: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`; fresh actual processes used SDK Go 1.26.7 on Linux/amd64, rather than rewritten expected fixtures.
- The source-to-final delta contains only two retained proof JSON files. Independently verified all 45 Hermes and 43 parent-reference source hashes, their exact identity with retained source-commit evidence, and the actual Rust CLI binary SHA256. All 25 files changed against the parent, including evidence, are hashed in `/tmp/symaira-usage768-jwt-independent-review-receipt.json`.

## P2: preserve hidden Go slice slots across nonempty shrink and regrowth

Location: `rust/symbrain-usage/src/provider_config/hermes.rs:95`, `providers.truncate(length)`.

The Rust decoder destroys provider elements when a duplicate nonempty `providers` field becomes shorter. Go's actual `encoding/json` decoder shortens the slice while retaining its backing elements. If a later duplicate field grows within that backing storage, an empty object or null element leaves prior struct fields intact. The newly enabled native path consequently loses a credential accepted by the existing Go CLI.

Minimal exact valid input, with no newline:

```json
{"providers":[{"id":"other"},{"id":"nous","access_token":"retained"}],"providers":[{"id":"other"}],"providers":[{},null]}
```

Exact input SHA256: `4c6cf6f6953002c13ae94c63074198add507f8000eff24e37727abe937bdc59f`.

Independently run immutable Go reports Nous configured=true, file credential available, and sends `Authorization: Bearer retained` to the owned transport. Candidate Rust reports configured=false/missing and sends no request. This changes behavior for a valid previously supported auth file after the broad Hermes Go gate is removed.

The reproducer copied the runner to `/tmp/hermes768-independent-shrink-run.sh` and inserted `/tmp/hermes768-independent-shrink-input.py` to replace only the existing `file-reuse-provider-slice` owned case with this input. All 80 unique IDs and fresh Go invocation remained; no candidate source or comparator was modified. The native full-report comparator fails as intended with exit 101, zero passed and one failed parent. This is a substantive candidate failure, not one of the five deliberate comparator negative controls.

Preserved evidence:

- `/tmp/symaira-usage768-jwt-independent-shrink-process.json`: rejected actual-process run at final immutable HEAD.
- `/tmp/symaira-usage768-jwt-independent-shrink-process.evidence/input.json`: full owned source corpus containing the exact trigger.
- `/tmp/symaira-usage768-jwt-independent-shrink-process.evidence/go.json`: actual Go token, request and report observations.
- `/tmp/symaira-usage768-jwt-independent-shrink-process.evidence/native-constructors.log`: actual native mismatch, including both reports.
- `/tmp/symaira-usage768-jwt-independent-shrink-process.log`: complete run log.

Required correction: model the visible Go slice length separately from retained backing slots and preserve Go allocation/reuse behavior when regrowing. Keep explicit `providers:null` and `providers:[]` reset behavior. Select Nous only from the final visible slice. Add actual-Go regressions for nonempty shrink/regrowth, omitted/null fields, reset/regrowth and hidden-entry visibility; retain the exact failure as evidence and rerun the complete strengthened gate. The ADR/matrix claim of slice-reuse parity needs the corrected implementation and proof.

## Independent verification performed

All lifecycle runs used `/tmp/symaira-subreaper.py`, umask 022, explicit pinned Go SDK PATH, disposable oracle HOME/XDG roots, the candidate's owned target, `CARGO_INCREMENTAL=0`, and debug-disabled dev/test profiles. No operator state, live remote provider requests or host Keychain credentials were used.

- Fresh existing Hermes corpus: **80/80** full constructor reports, authorization request observations and no-write comparisons; **90/90** actual CLI stdout/stderr/exit comparisons; **5/5** required negative controls. Receipt: `/tmp/symaira-usage768-jwt-independent-process.json`, raw evidence beside it under `.evidence/`, complete log under `.log`.
- Four owned Unix filesystem observations ran against actual binaries. Symlink and directory absence behavior matched. FIFO deliberately records actual frozen Go timeout and bounded native missing-credential behavior; this is an explicit safety contract difference, not a parity pass or Windows proof.
- Fresh parent credential gate: **44/44** constructor/request comparisons and **110/110** actual CLI byte/exit comparisons, with all **5** controls. Receipt: `/tmp/symaira-usage768-jwt-independent-parent-process.json`, raw evidence and log beside it.
- Ordinary `cargo test --locked -p symbrain-usage`: **82 passed, 0 failed, 2 ignored**, six complete summaries. Both ignored oracle integration parents were separately and explicitly executed by the fresh gates above. Ordinary log: `/tmp/symaira-usage768-jwt-independent-usage-tests.log`.
- Two focused actual CLI routing tests passed separately, one parent each, 19 filtered each: all historical Hermes file shapes and native routing versus numerical-overflow Go fallback; supported Kimi/Hermes home overrides. Log: `/tmp/symaira-usage768-jwt-independent-cli-routing-tests.log`.
- Strict all-target Clippy for Usage and CLI, workspace format, workflow actionlint and runner shell syntax passed independently. Clippy log: `/tmp/symaira-usage768-jwt-independent-clippy.log`.
- Expected control failures are not included in positive Rust counts: three Cargo integration controls require failure and the intended diagnostic; two CLI controls execute actual CLI processes then deliberately corrupt the comparator's observed exit or corpus in memory. The new shrink/regrowth failure is distinct and blocks acceptance.

The baseline corpus passing does not override the additional independently reproduced regression. No broader suites were duplicated after these scoped checks passed.

## Full-layer inspection account

All 23 changed non-evidence files plus the two retained reports were inspected, including production parsing/routing, surrounding actual Go implementation, constructor/transport integration, harness, control handling, workflow, manifests and documentation:

- `.github/workflows/ci.yml`: identical named Hermes actual-process gate on the three native OS jobs; artifact upload retains complete receipt and raw evidence directory on failure. Native OS execution is still required on the eventual correction head.
- `Cargo.lock`, `rust/symbrain-usage/Cargo.toml`: add existing pinned workspace `cap-std` dependency wiring, without unrelated registry or Git pin changes.
- `rust/symbrain-usage/src/provider_config.rs` and new `provider_config/hermes.rs`/`jwt.rs`; extracted `kimi_nous.rs`; modified `routing.rs`; `provider_config_tests.rs`.
- `rust/symbrain-cli/src/usage_cli.rs`, `rust/symbrain-cli/tests/mcp_cli_tests.rs`: narrow routing boundary and actual CLI expectations. The production CLI file changes are explanatory comments; functional authority stays in Usage routing.
- `rust/symbrain-usage/tests/hermes_credential_tests.rs`: exact distinct source/observed corpus checks, complete reports and requests, source auth-byte comparisons, explicit oracle invocation instead of self-approval.
- Every `scripts/usage-hermes-oracle/` file: `README.md`, `cases.py`, `cli.py`, `controls.py`, `filesystem.py`, `provider_test.go.txt`, `receipt.py`, `run.sh`.
- `docs/adr/usage-hermes-file-jwt-768.md`, `migration/contract-matrix.csv`, `migration/implementation-plan.md`, retained source-head and final proof reports.

Read-only file access is capability-rooted at the chosen Hermes parent, matching the existing Go selected-root boundary. Unix final entry is no-follow/nonblocking and must be regular; read size is capped at 64 KiB with a bounded growth check. The code does not refresh, write, lock or persist auth, does not resolve secret-looking file strings, and preserves environment precedence and first matching Nous provider priority. The selected parent itself is the authorized root; this does not imply an independent ban on selecting a linked parent. Non-Unix final-link semantics require native proof.

Ordered JSON fields preserve duplicate/case-fold processing and Go-compatible invalid UTF-8 normalization. Known field type errors invalidate the credential; unrelated metadata can remain outside native floating-point range without failing ignored fields. The identified slice backing regression is the remaining defect in the reviewed provider reuse logic.

JWT payload parsing only checks presence/expiry for existing credential selection; it does not authenticate a JWT or grant local authority. Actual HTTP transport retains provider authorization. The frozen Go `Exp` field is a `*float64`, so null correctly resets expiry; scalar-null semantics must not be substituted here. Raw URL base64 CR/LF and tail-bit handling, expiry truncation/current-time comparison, duplicate claim processing and ignored metadata were checked against the actual Go source and corpus. Finite numeric-to-int overflow retains an explicit Go gate rather than claiming architecture-independent parity.

Usage remains the credential/account domain owner. The patch does not couple the standalone parser to Brain gateway/MCP authority, Guard grants, Audit policy or secret storage. Surrounding provider URL checks, non-Hermes file gates, trusted HTTP request construction, report redaction and existing Keychain boundary remain in place. No frozen Go production or migration fixture was changed.

## Scope and acceptance boundary

Issue #768 remains open: numerical overflow cases, remaining non-Hermes file families and host Keychain behavior still require their documented Go boundaries or later ports. The Unix FIFO safety deviation is expressly documented. Linux observations do not establish Windows reparse/link semantics or macOS native Keychain behavior.

Correct the P2 regression, independently rerun the expanded actual-process gate against the new immutable source, refresh all three native OS CI jobs, and retain source-bound evidence before scoped approval. This report does not authorize retiring the residual Go surface or closing the full issue.
