# Copilot and Kimi credential-file contracts (#768)

## Decision and reason

Integrate independently approved provider-files parentfdfea204 normally and keep
its frozen-Go, original failures and native source receipts historical. Preserve
the original97-case Go-only baseline exactly. Before reusing its released target,
archive every actual native ELF executable:112 paths/74 unique binaries, byte and
SHA256 verified under `/workspace/oracles/symaira-usage768-files-f241-binaries/`.
The archive receipt binds sourcef241/7bd and the original three process manifests,
including CLI9aed2a28060fe84f945db937def4ed10c83f13b15a008d4720ebebf3cd853f38.
Only `/workspace/symaira-usage768-local-files/target` is now used for this lane.
Parent source and evidence are unchanged. This avoids a duplicate build target
while preserving actual prior binaries for review.

Copilot files follow Go's raw root map: exact root keys, last duplicate entry
replacement, then typed per-entry scalar parsing with Go field folding and null
retention. Invalid entries are skipped; malformed or empty apps fall through to
hosts. The `github.com:` pass precedes fallback entries. One token, or multiple
identical eligible tokens, has a deterministic result. Different tokens within
the first usable pass retain Go because map iteration is unspecified. A single
observed Go selection never proves a deterministic account choice. Environment
presence, including resolution failure, preempts Copilot files as in Go.

Kimi uses the same ordered typed-string decoder for access_token. Unknown refresh
or other metadata is ignored, scalar null retains an earlier token, and a wrong
known type invalidates the complete file even before a later valid duplicate.
Root null/invalid input means missing. Current/legacy/explicit-home precedence,
API/CLI/web strategy order and CLI-preferred AuthStatus stay unchanged. Literal
reference-shaped file tokens stay literal; only environment sources resolve via
the accepted credential broker. No in-process secret store or master key appears.

Both files use the existing capability-rooted, bounded, read-only common reader
and Go10000-container limit. Typed metadata may contain1e1000, unlike generic
Codex maps. Regular files over64KiB supply no token, following Go; Copilot then
tries hosts and Kimi remains missing. Unreadable/nonregular/symlink access keeps
the conservative file gate. Device bytes use the same safe reader; regular
oversize device files are ignored in Go and natively. Unproven non-ASCII or
invalid-UTF8 device values retain Go. The existing Windows HOME mismatch,
unsupported base/workspace, numeric expiry and automatic host-Keychain gates
remain. Host ACL and Windows root-confinement proof are not inferred from mocks.

## Evidence requirements and limits

Retain66 file inputs from the original97 baseline and append20 actual-Go cases:
wrong known types, duplicate-map replacement, null/array roots, ignored refresh
metadata, literal references, exact10000/10001 depth boundaries,65536/65537 size
boundaries and credential-resolution error priority. The native gate requires86
distinct cases, full reports/complete request byte comparison for81 eligible
Linux/macOS cases (80 on Windows), read-only checks for every input, and explicit
retained eligibility checks for the remainder. It never counts those gates as
full constructor parity. Actual private CLI byte/exit comparisons and five real
failing replay controls are mandatory; no CLI positive provider request runs.

The expanded implementation exposed an older unit fixture claiming GitHub-prefix
priority for the bare key `github.com`. Go only prioritizes `github.com:`; bare
GitHub and another token belong to its unspecified fallback map iteration.
Correct that unit fixture to the actual prefix while retaining exact winner
assertions and the real ambiguous-map gates. Historical fixtures/routing flags
are preserved; current assertions derive deterministic results from frozen Go
outputs and the new process evidence.

This checkpoint must pass the new constructor/request/CLI/control gate plus all
three complete accepted parent process gates on one clean source, ordinary
all-features CLI/Usage tests, strict Clippy/fmt and native CI syntax checks. Native
macOS/Windows exact-candidate CI and independent review remain required. Linux
validation and authorship do not establish approval or full #768 completion.
