# Independent full-layer review: Usage #768 Claude/Codex provider files

**Disposition: REQUEST CHANGES — one inherited P2 affecting both Hermes JSON entry points.** The new Claude/Codex file implementation has no separate actionable finding from this review. Its positive evidence does not close the shared Hermes parent-contract gap independently reproduced below. Issue #768 remains open; current native macOS/Windows/Linux CI is also required.

## Immutable artifact and independence

- Worktree: `/workspace/symaira-usage768-files`.
- Reviewed clean final HEAD: `5111d8b66246f1c397fc9f962cece73c10929ad2`.
- Production/source checkpoint: `52a089c631bea00a47574bed40c42199e3ceecd9`.
- Normal main base: `31de72294521701fc4b1a0bce39f03cc34d72e7e`.
- Previously reviewed Usage parent: `c7a7cb4acdc127f69db8fd6f9caf42d831dd7c79`; Hermes slice repair ancestor: `9b37c53f43681ee0d5ff90ec6a7f98f2f8279c3f`.
- Actual frozen Go oracle: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, rebuilt by each real runner with Go SDK `go1.26.7 linux/amd64`; Rust `1.98.0`.
- Final source-to-HEAD delta is exactly three new process-evidence JSON files. Candidate is still clean. No production Go or frozen fixture changed relative to main.

This reviewer independently inspected code, harness, CI and documentation, reran all three actual-process gates, reran the affected ordinary tests/checks, and supplied additional source-bound inputs to unchanged real production constructor/report replays. The author did not implement this review or its probes. No source modification, GitHub write, real credential, host-Keychain access, provider endpoint, or child-agent review was used. Existing candidate-owned target was exclusively reused after author coordination; all lifecycle processes ran under `/tmp/symaira-subreaper.py`, umask 022, incremental/debug disabled. Additional runners and inputs are outside the candidate in `/tmp`.

## Finding P2: apply the Go JSON depth limit to Hermes files and decoded JWT claims

Locations: `rust/symbrain-usage/src/provider_config/hermes.rs:13-15` and `rust/symbrain-usage/src/provider_config/jwt.rs:21-22`.

The shared `go_json_credential_limits` helper now enforces Go's 10,000-container syntax limit for Claude/Codex. Hermes's outer document instead deserializes into `CredentialFields` without that check. JWT expiry deserializes unknown claims through `IgnoredAny` without that check. These Serde raw/ignored-value paths skip deep metadata iteratively and therefore accept a depth of 10,001, whereas Go rejects the complete JSON input. Both entry-point gaps are inherited from the previously reviewed Hermes parent, rather than a newly introduced Claude/Codex regression. They are nevertheless reachable through the current native route, and were not detected by the original 92-case parent gate.

Two independent actual-process failures establish the behavior:

1. Outer auth file: `{"providers":[{"id":"nous","access_token":"retained"}],"ignored":` followed by 10,000 `[` characters, `0`, 10,000 `]` characters and the closing object. Root plus arrays gives 10,001 containers. File SHA256 `c4894ae437b09aad4de0bccdda763f0a5d3aa5318acba869c2e79eb7e06f9041`.
2. Decoded JWT claims: `{"exp":4102444800,"ignored":` followed by the same arrays and closing object, encoded with Go-compatible raw URL base64 inside a three-part `invoke_jwt`. The credential file itself has shallow nesting; the decoded claims reach 10,001 containers. File SHA256 `0daf023944919f427ae57862f656ea3fbe4e68590cc5f634a9e1af1f01156707`.

In each case, actual frozen Go reports `configured=false`, missing credentials and no Authorization request. Actual native code reports `configured=true`, auth available from `file`, with a fallback error in the complete report. Each genuine comparison exits 101 with `0 passed; 1 failed` at the complete-report assertion. These are substantive mismatches, not intentional rejection controls. The assertion stops before the subsequent native header comparison, so this finding does not assert a separately observed native Authorization header or any real network access.

For both entry points, changing the arrays to 9,999 preserves a maximum total depth of exactly 10,000. Each accepted-boundary variant independently passes the entire 92 constructor/report, 100 CLI and five-control gate, with identical source and actual Go/native CLI binary hashes. All test files remain below the 64 KiB credential bound. The overflow fails despite native eligibility accepting the route.

Exact rejected/accepted evidence:

- `/tmp/symaira-usage768-files-independent-hermes-depth10000-process.json` and matching `.log`/`.evidence/` directory: rejected by Go, mismatched by native.
- `/tmp/symaira-usage768-files-independent-jwt-depth10000-process.json` and matching `.log`/`.evidence/`: rejected by Go, mismatched by native.
- `/tmp/symaira-usage768-files-independent-hermes-depth9999-process.json` and `/tmp/symaira-usage768-files-independent-jwt-depth9999-process.json`: accepted complete boundary replays.
- Exact hex inputs: `/tmp/usage768-files-independent-hermes-depth-{9999,10000}-input.json` and `/tmp/usage768-files-independent-jwt-depth-{9999,10000}-input.json`.
- Reproducible reviewer input writers/runners: `/tmp/usage768-files-independent-{hermes,jwt}-depth-input.py` and `/tmp/usage768-files-independent-{hermes,jwt}-depth-run.sh`. Each replaces only one owned existing case, retaining the original 92 distinct IDs and all normal comparator assertions; the JWT runner requires `REVIEW_JWT_DEPTH=9999` or `10000`, and the outer-file runner requires `REVIEW_HERMES_DEPTH` accordingly.

Correction: reject depth above 10,000 at both complete input boundaries, using the shared bounded iterative helper with typed unknown-number semantics preserved. Add explicit accepted/overflow cases for both file JSON and decoded JWT JSON. Preserve the current failures and all older slice/help evidence; then rerun the full files, Hermes and reference gates plus fresh native three-OS CI. This is a report/credential-selection correctness finding; expiry parsing remains an unverified credential-presence check, not authentication authority.

## Full-layer inspection and positive results

The 68-file main-to-source range includes the previously independently reviewed reference/Hermes implementation and preservation evidence. The direct Provider Files delta was reviewed against c7, with accepted incoming main Activity changes distinguished from Usage changes. Source hashes for all changed main-to-source inputs and the scoped direct delta list are retained in the accompanying receipt.

Inspected production layers include `provider_config.rs`, the shared ordered raw JSON reader/visitor, new `claude_file.rs` and `codex_file.rs`, changed `accounts.rs`, `files.rs`, `hermes.rs`, `routing.rs`, unchanged parent `jwt.rs`, Unicode/field normalization, environment/reference resolution, source selection and request/report provenance. Claude typed map merges, duplicate/null behavior, Unicode field folding, default-account preference and nondeterministic distinct-token fallback match actual Go. Codex retains exact-key generic map replacement, top-level-token precedence, every-number finite float64 validation and unknown overflow rejection. The new shared helper's depth behavior is proved for the new two provider parsers. Literal file references remain literal credentials; environment references retain their existing scoped resolver.

Reader checks include explicit read-only capability-rooted opening, final-link refusal/nonblocking Unix flags, regular-file metadata, pre-read and growth bounds, absence on errors, and no refresh/rewrite/locking. The selected ambient parent reflects the user/provider-root contract. Automatic Claude Keychain remains the existing private adapter and fallback, not a new gateway capability: private empty injected readers prove source ordering and exact calls, not host inventory/ACL parity. CLI unresolved OAuth sources preempt host-Keychain access; the separate full file constructor/report gate supplies positive credential cases. Conservative gates remain for unproven provider families, distinct nondefault Claude tokens, architecture-sensitive numeric JWT expiry and Windows home disagreement. Production ownership does not expand vault permissions or MCP authority.

Inspected test/harness layers include every file in `scripts/usage-provider-files-oracle/`, its actual Go supplemental constructor/report seam, native private oracle test, changed config/CLI tests, parent Hermes/reference integration replays and helpers. They check exact distinct input coverage, complete reports, literal auth headers, no store mutation and deliberate failed replays. New production files remain under 400 lines. Bounded file reads, iterative ignored metadata scanning, finite input size and exclusive lifecycle ownership were checked; no additional performance/stack finding was established.

Independent clean positive replay counts:

| Gate | Constructor/source observations | Actual CLI bytes/exit | Actual negative controls |
| --- | --- | --- | --- |
| Provider Files | 86 distinct inputs; 85 full reports plus one retained actual ambiguous-account gate | 151/151, including 85 invalid-flag native-route comparisons and 66 credential-free table/JSON comparisons | 5/5 |
| Hermes parent | 92/92 complete reports/requests | 100/100 | 5/5 |
| Environment/reference parent | 44/44 constructor/report/request cases | 110/110 | 5/5 |

Process receipts are `/tmp/symaira-usage768-files-independent-process.json`, `...-hermes-process.json` and `...-reference-process.json`, each with raw owned inputs, real Go/native observations, CLI/control/filesystem records and stage logs in its `.evidence` directory. All report `candidate_dirty=false` and exact final HEAD5111. Ordinary affected suites independently pass **355 tests, zero failures, three ignored source gates, 35 summaries**; each ignored source gate was explicitly run above. Strict affected-package Clippy including all targets, workspace format check and CI actionlint also independently pass.

All 53/51/47 receipt source-manifest hashes match candidate files and authored source52a receipts. All 44 source entries shared across the three manifests agree. All three actual CLI receipts share freshly gate-built Go SHA256 `0654376cf8fc33c3be0b03bd394b665842d04e6659b3dfd67c0d6786d2f136e2` and native SHA256 `80cafb91e627b69ce4c6338c3adb46d2a8aea4df58d14e0be734206080832d24`; actual native binary hashes identically after validation. The separate archived Go artifact has a different hash and was not substituted for this gate build.

Four owned Unix filesystem observations for Codex, and the retained four Hermes observations, execute actual Go/native CLI processes. Symlink/directory outcomes match; FIFO records preserve actual Go timeout against immediate bounded native rejection as an explicit safety difference, not exact parity or a Windows claim. The new workflow invokes the file gate with explicit bash on all three native jobs and always uploads result/raw evidence; actionlint passes. ADR, README, migration ledger and matrix explicitly retain native CI/host integration and unproven provider limits rather than closing full #768.

## Preservation and remaining acceptance

All 14 prior c7 environment/Hermes evidence files are byte-identical, including the original273 slice-backing finding and corrected review/proofs. Initial actual help failure is preserved in tracked `initial-help-byte-mismatch.json`, and the original 151 observations remain in `/tmp/symaira-files768-first-full.evidence/cli.json` (66 passed, 85 failed). Restored native help bytes now pass fresh actual Go comparisons. No historical failure or approval report was rewritten to hide these new depth failures.

The accompanying `/tmp/symaira-usage768-files-independent-review-receipt.json` binds every source, report, extra input/runner, raw evidence and validation log to hashes. Candidate remains unchanged, and a final `/proc` executable/CWD/open-FD scan found no process using its owned target: target is released to the author. The one finding needs correction and independent verification. Native three-OS exact-head CI, real host-Keychain permission behavior, remaining Go-owned provider/source families, out-of-range expiry behavior and full #768 completion remain separate open acceptance work.
