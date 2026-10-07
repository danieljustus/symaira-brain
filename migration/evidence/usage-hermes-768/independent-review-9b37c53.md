# Independent corrected full-layer review: Usage Hermes/JWT #768

Disposition: **APPROVE the scoped correction**, subject to fresh required native three-OS CI on the eventual published head. No actionable findings remain in the inspected scope. The P2 independently found at the original immutable candidate is closed by fresh actual-process proof; the original report and failure remain preserved. Issue #768 stays open for documented remaining migration gates.

## Exact immutable inputs

- Worktree: `/workspace/symaira-usage768-jwt-fix`, clean final HEAD `9b37c53f43681ee0d5ff90ec6a7f98f2f8279c3f`.
- Corrected production/harness/docs source: `1dad2f7bf25228eda67238fdbd544b85ebfbdf1d`.
- Parent base: `117e4f37f6ce1c669e9856bcb90e41d8611913ab`.
- Previously reviewed, rejected candidate: `273a93c03d0cc526d81d962ac137a5a6fb0d73e9`, source `63e2527c5bbec48643495d4a169bbdd3d50b6a47`.
- Actual frozen Go reference: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`; fresh Go SDK 1.26.7 on Linux/amd64 and Rust 1.98.

The corrected source-to-final delta is only the two Linux process proof files. Independently verified all **47 Hermes / 43 parent-reference source hashes**, their identity with the retained source-commit manifests, and actual CLI binary hashes. A separate review receipt hashes all **27 changed files** against the parent, including documentation, workflow and evidence: `/tmp/symaira-usage768-jwt-fix-independent-review-receipt.json`.

Reviewer changed no candidate source, fixtures or evidence, and performed no push, PR, merge, issue closure or operator-state access. Builds reused only this candidate's existing target with incremental and dev/test debug disabled. Lifecycle execution used the subreaper, explicit pinned Go SDK PATH, umask 022 and disposable HOME/XDG roots.

## Original finding closure

The original parser truncated its stored provider vector after each duplicate array, destroying Go backing entries hidden by nonempty shrinkage. The correction retains visited slots, tracks final visible length separately, resets visited slots on null/empty arrays and searches only the visible prefix. Thus nonempty shrink/regrowth preserves omitted/null struct fields, while hidden providers cannot participate in selection until visible again.

This design was independently checked against the pinned SDK source: `encoding/json/decode.go` grows and sets visible length while decoding, truncates length without clearing slice elements, and gives an empty array a new zero-capacity slice. `runtime/slice.go:reflect_growslice` explicitly preserves old capacity contents even during reallocation. Previously unvisited slots are zero/default values, so Rust's allocator capacity need not reproduce Go's allocation sizes. Retained populated slots are bounded by the already bounded 64 KiB input; the correction introduces no unbounded storage or I/O.

The exact original failing input now passes the actual fresh Go/native complete-report/request/no-write comparator:

```json
{"providers":[{"id":"other"},{"id":"nous","access_token":"retained"}],"providers":[{"id":"other"}],"providers":[{},null]}
```

Both implementations select `retained`, report the file credential configured/available and observe `Bearer retained` in the owned transport. Exact input SHA256: `4c6cf6f6953002c13ae94c63074198add507f8000eff24e37727abe937bdc59f`.

Thirteen independently supplied probe inputs were run in addition to the committed corpus: the original trigger plus twelve varied shrink/regrowth cases. These use larger 9–64-slot histories, regrowth to 81 entries across allocation boundaries, repeated cycles, mixed `providers` case, null scalar no-ops, null/empty reset, explicit token clearing, ID replacement, hidden-tail exclusion, invoke-token priority and newly visited tail values. All thirteen matched actual Go reports and authorization requests with unchanged auth bytes. They replace thirteen selected entries in a separately executed 92-entry replay, preserving corpus cardinality and comparator assertions; they are not claimed as 13 newly appended corpus IDs or 13 separate Rust test parents.

Independent inputs and complete evidence:

- `/tmp/symaira-usage768-jwt-fix-independent-regrowth-inputs.json`: exact thirteen JSON values and hashes.
- `/tmp/symaira-usage768-jwt-fix-independent-regrowth-process.json`: full source-bound fresh replay receipt.
- `/tmp/symaira-usage768-jwt-fix-independent-regrowth-process.evidence/`: actual Go input/report/request observations, native comparison and control logs.
- `/tmp/hermes768-fix-independent-regrowth-run.sh` and `/tmp/hermes768-fix-independent-regrowth-input.py`: reviewer-owned runner copy and input modifier; production comparator unchanged.

The original `/tmp/symaira-usage768-jwt-independent-review.md`, original failure receipt/log/raw evidence and their recorded hashes remain unchanged. The new tracked `migration/evidence/usage-hermes-768/linux-slice-regrowth-failure.json` includes the actual original Go record; independently verified its exact identity with the retained failing replay. Also verified the first original eighty input records remain unchanged in the new 92-case corpus.

## Independently executed verification

- Committed corrected Hermes gate: **92/92** complete constructor/report/request/no-write cases, **100/100** actual CLI table/JSON stdout/stderr/exit comparisons, **5** correctly rejected controls. Fresh receipt `/tmp/symaira-usage768-jwt-fix-independent-process.json` and accompanying `.evidence/` and `.log`.
- Separate reviewer-variant Hermes replay: **92/92**, **100/100**, all **5** controls, containing the thirteen independent inputs described above.
- Parent credential gate: **44/44** constructor/request comparisons, **110/110** actual CLI comparisons, all **5** controls. Fresh receipt `/tmp/symaira-usage768-jwt-fix-independent-parent-process.json`, with raw evidence/log alongside.
- Four owned Unix filesystem observations ran freshly for each Hermes gate. Symlinks and directory absence behavior match actual Go; FIFO explicitly retains frozen Go timeout versus bounded native missing credential as a documented safety deviation. It is not counted as byte parity or native Windows proof.
- Ordinary Usage crate suite: **82 passed, 0 failed, 2 ignored**, six complete summaries. Both oracle parents ordinarily ignored by this suite ran explicitly in the fresh gates; no oracle approval is inferred from ignored tests.
- Two targeted actual CLI routing parents passed, one each, with 19 filtered each: historical Hermes file shapes/overflow fallback and supported Kimi/Hermes home overrides. No failed/ignored parent in these executions.
- Strict all-target Usage/CLI Clippy, workspace format, workflow actionlint and Bash syntax checks passed. Initial bare `actionlint` invocation was absent from PATH; the installed `/workspace/toolchains/bin/actionlint` was then explicitly executed successfully. This tooling retry did not affect source or process proofs.

Logs are `/tmp/symaira-usage768-jwt-fix-independent-usage-tests.log`, `...-cli-routing-tests.log`, `...-clippy.log` and `...-validation.log`; their hashes are recorded in the independent review receipt. Expected negative controls are kept separate from positive test totals: three actual Cargo failures require the intended diagnostic; two CLI controls run actual commands then intentionally mutate comparator observations. They are not five production CLI failure injections.

## Full changed-layer disposition

All changed production layers, surrounding Go semantics and trust boundaries from the original review were checked against the corrected immutable source, with the complete correction delta inspected. The full parent-to-correction source file set is hashed in the receipt; the examined layers are:

- Usage `provider_config.rs`, `hermes.rs`, `jwt.rs`, `kimi_nous.rs`, `routing.rs`, provider-config tests, and surrounding file/env normalization, literal secret-reference handling, account constructors, HTTP authorization/report redaction and actual frozen Go Nous/credential implementations.
- CLI Usage adapter and actual CLI routing tests; the new correction changes only the Hermes provider decoder's retained-slot model in production. Existing numerical overflow and non-Hermes routing boundaries remain.
- `hermes_credential_tests.rs` and all Hermes runner/cases/CLI/controls/filesystem/supplemental-Go/receipt scripts. Strengthened assertions require 92 distinct source/observed IDs and full reports/requests/no-write proof; 100 CLI comparisons retain all required byte/exit checks. Original eighty cases were not replaced in the tracked corpus.
- Workspace dependency manifests/lock: the capability-root reader uses already pinned workspace `cap-std`; no unrelated external pin updates.
- Three native OS CI jobs and artifact retention: source-bound gate and complete raw evidence upload remain required on each platform.
- README, ADR, migration plan, contract matrix and three retained evidence files. The source-commit receipts are accurately distinguished from evidence-only final HEAD. The original independent rejection is preserved, and remaining issue gates are stated rather than reported complete.

The capability-rooted read-only Hermes reader still chooses the same parent-root boundary as Go, rejects final Unix links/special files, caps both metadata and actual read size, and performs no auth refresh, writes, locks or secret-provider resolution of file literals. Parent-root selection is authorized path selection, not a claim that arbitrary parent symlinks are forbidden. Non-Unix confinement requires native CI proof.

Ordered typed JSON/string normalization, duplicate/case-fold semantics, unknown metadata and invalid known-field behavior remain as reviewed. JWT payload handling is an unverified expiry/presence check, not an authentication or permission decision; actual HTTP authorization stays with Usage transport. Expiry null follows the actual Go pointer field contract. Architecture-sensitive finite numerical overflow remains on Go. The correction does not expand Brain MCP authority, Guard grants, Audit policy or secret storage access.

## Remaining scope and acceptance

No additional actionable findings were found. This is scoped approval of the corrected Hermes migration, not full completion of #768. Numerical overflow, other credential-file families and host Keychain behavior retain their documented gates. Fresh Linux proof does not establish Windows reparse/link semantics or macOS host integration. Require current native three-OS CI on the final published head before integration.

At root's resource-management request, independently verified original candidate HEAD/clean state, target ignore status, absence of processes referencing the old target, and resolved directory identity. Archived eight actual old CLI/test binaries and SHA256 values to `/workspace/oracles/symaira-usage768-jwt-273-binaries/receipt.json`, then removed only `/workspace/symaira-usage768-jwt/target` through guarded Python. Archive hashes were checked again; original source and failure evidence are intact. No other target was removed.
