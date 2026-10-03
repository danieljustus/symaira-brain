# Hermes typed JSON depth boundary (#768)

The full independent review of immutable provider-files `5111d8b6` found one
inherited P2 defect: Hermes credentials and decoded JWT claims omitted the shared
Go JSON nesting limit. At 10001 total containers, frozen Go reports configured=false and missing
credentials without a request. The original Rust report instead has
configured=true, file credentials available and a fallback error. The full report
comparison failed before the subsequent header comparison, so the original
replay does not separately establish Rust Authorization bytes or a request. Both entry points accepted the exact 10000
boundary. The newly introduced Claude/Codex readers had already used this guard.

Apply `go_json_credential_limits(text, false)` to normalized text before either
Hermes typed decoder. This reuses the bounded iterative scanner and does not
convert unknown metadata numbers to float64: Go's typed readers still ignore
valid unknown `1e1000` values. Unknown deep arrays must obey the global depth
limit even though typed decoding ignores their contents. A rejected file or
claim remains missing, with no request and no numeric-overflow fallback route.
The existing architecture-dependent numeric expiry gate remains unchanged.

Append four exact independent boundary inputs to the supplemental process
corpus: outer file and decoded JWT payload, each at 10000 accepted and 10001
rejected containers. Preserve all original 92 inputs and IDs verbatim, including
the unknown numeric overflow cases and slice backing-slot regressions. The gate
now requires 96 full reports/auth-header/no-write checks and 104 actual private
CLI stdout/stderr/exit comparisons; both new rejected cases run table and JSON
output. Its five real failing replay controls remain mandatory. Frozen Go and
historical fixtures are unchanged.

The complete original review, receipt, two actual failed comparisons, two
passing 92/100/5 boundary gates, private inputs and their partial/full evidence
are retained byte-for-byte under
`migration/evidence/usage-hermes-depth-768/original-review-5111/`.
`retention-manifest.json` maps original paths to tracked paths and SHA256 hashes.
Historical accepted scopes remain historical: these retained failures explain
why their earlier corpus did not establish this boundary.

This focused correction must receive fresh source-bound provider-files,
expanded Hermes and credential-reference gates, ordinary CLI/Usage tests and
strict checks before review. Native macOS/Windows evidence remains required;
Linux results do not establish those targets. The Go-only Copilot/Kimi baseline
is an independent future lane. No additional credential family or host Keychain
cutover, full #768 closure or publication follows from this correction.

## Corrected Linux verification

Clean source `7bdde611aef63b1700fed5cbc12d5ce2bafe7a4d` passes all three complete
process gates: Hermes96 reports/104 CLI/five real rejection controls;
provider-files86 inputs (85 full comparisons and one retained ambiguity gate)/151
CLI/five controls; references44 full reports/110 CLI/five controls. All source
manifests match current bytes and their44 common entries agree. The exact10000
boundary remains configured with one canned authenticated request; both10001
cases are missing with no request and require no Go routing fallback.

The ordinary CLI/Usage all-features suite passes355 tests, zero failures and three
ignored fresh process oracles; all three oracles were explicitly executed above.
Strict all-targets/all-features Clippy, workspace and included-fragment formatting,
CI actionlint and diff checks pass. Frozen Go and historical fixture diffs are
empty. Reports and verification logs are retained separately as
`migration/evidence/usage-hermes-depth-768/linux-corrected-*`; original5111 proof
is preserved. The final evidence/documentation commit leaves every validated
production/comparator manifest hash unchanged. Independent corrected review and
native macOS/Windows exact-candidate gates remain required.
