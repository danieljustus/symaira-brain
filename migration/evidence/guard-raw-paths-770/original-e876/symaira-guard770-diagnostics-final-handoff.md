# Guard770 diagnostics author handoff

Clean immutable candidate `e8766e354b4f6b21617d4201de23a96d65ac47c7` in
`/workspace/symaira-guard770-diagnostics`, branch `issue/770-guard-native-diagnostics`.
Validated source `43caaaff86e1be902c80c05aa6e4c15e028a8da1`; final successor is
strictly new clean43 evidence. Normal merge of approved published Guard
`2f50ae75f10fba200bc1cfbb6892da0b64651740` is
`2c9f264a54fc9eab4ee4b78ab404ab7b9b6e5c53`.

Receipt `/tmp/symaira-guard770-diagnostics-final-handoff.json` SHA256
`10ba42164cf681e0a1171b0416869b883494aa15d50ff1e1547b33782d733bc9`.
This is author evidence for independent full-layer review, not approval.
No push, PR, issue closure, Go removal or host-credential access occurred.

The earlier dirty94-case/89-match/4-gated/1-audit-difference checkpoint remains
byte-exact: five source snapshots, original patch, process report and21-test/lint
logs in `migration/evidence/guard-diagnostics-770/original-wip-4a9`, committed before
normal integration at4f772a0. Original published2f50 worktree/source/evidence/target
are untouched. The diagnostics lane already owned184MB of target artifacts;
that explicit authorized alternative was reused and grew to203MB. Published
Guard target was never reused, moved or deleted; no new duplicate target was made.

## Implementation and rationale

- Config decoding separates all known TOML type/shape checks from semantic
  validation. This preserves Go's decode-before-validation ordering. Single
  invalid defaults, ordered rules, enabled sequence zero→three/negative/one,
  and spawn missing/relative paths are native, with exact quoted diagnostics.
  Defaults with several invalid entries retain a gate because Go map order is
  random; unknown-key warning/library error text also remains explicitly gated.
- Audit anchors now use a small ordered raw-field decoder after the existing
  Go syntax scanner. It preserves duplicate/null/folded fields and first-error
  ordering; known numeric errors retain exact literal spelling. SchemaVersion
  uses target int width. Unknown values are skipped without unwanted float64
  overflow or128-depth assumptions. Unsupported string replacement remains a
  conservative gate. Thirty actual cases include top-level wrong types, integer
  endpoints/overflow/decimals/exponents, schema width, ASCII/long-s folding,
  duplicate ordering, unknown1e309 and150 nested unknown arrays.
- The Guard adapter formats the actual Unix EISDIR audit-open error as Go does,
  without any ambient metadata query or reopen. Every other error keeps its
  honest capability/system diagnostic. RawJsonlAppender and its directory
  capabilities/private modes/no-follow/nonblocking/short-write rules are entirely
  unchanged. Windows audit-open stage/spelling remains unported. Denial on audit
  failure is unchanged, and no audit record is published after failure.
- Guard-only handlers remain independent of Brain gateway/exposure/credential
  authority. Shared runtime64 inputs are byte-identical to approved2f50, including
  core/kernel/audit sources, root manifests/lockfile and supplemental Go entry.
  New production modules: config_decode212, anchor_decode96 and audit_error17
  lines; touched existing production components remain under400 lines.

## Clean source proof

The tracked root-CWD runner rebuilds actual unchanged Go public handlers from the
complete frozen DC tree using Go1.26.7. It executes all124 cases, without skipping
the unsupported states: current Linux121 complete byte/exit/filesystem matches,
three selected TOML fail-closed states and zero Unix audit wording differences.
All original94 cases remain; two previously weaker selected contracts are now
strict ordinary byte/state comparisons after actual proof. The original80
published baseline and every original Go source/fixture remain unchanged.

Three actual native-output mutants are rejected: hide a config error, hide an
anchor overflow error, or change audit-failure deny to allow. Each wrapper runs
the actual native binary and asserts the underlying expected exit/stderr before
modification. Windows retains its explicit deny-only audit-wording case; the
new full gate and controls are retained by all three existing native CI jobs.
No failure was converted into a passing ordinary equality comparison.

Component GuardCLI+kernel:62 passed/0failed/0ignored/12 summaries; strict
all-target/all-feature clippy, fmt and actionlint pass. Reports bind1217 frozen
Go inputs, supplemental entry and both actual process binaries. Final validation
binds88 runtime/test/harness/workflow inputs to immutable43 and verifies every
one remains identical at e876. The ordinary Brain adapter is source-unchanged;
its broader fresh process/frozen-doctor acceptance is required in native CI,
not claimed from the separate standalone proof or historical2f50 adapter tests.

Full clean reports/logs/SHA manifest:
`migration/evidence/guard-diagnostics-770/clean43/`.
Dirty intermediate failures/passing follow-ups remain in sibling `prototype`:
first number-to-string formatting difference, obsolete unit expectation failure,
first strict-lint failure and actual corrected124/3-control proof. All copies are
verified against the exact originals; originalWIP/clean proof provenance is
explicitly distinct. Final receipt lists53 actual ELF artifacts with SHA and
sizes without pretending every inherited build-tool artifact is a new test.

## Remaining scope and reproduction

Three selected unported cases are TOML decoder error text before validation,
multiple invalid defaults and unknown-key warnings. They do not inventory all
remaining syntax/type/inline/array/table representation, config/anchor I/O,
Unicode replacement, malformed discovery or output-failure branches. Standalone
returns explicit failure before stdout for unsupported states; the existing
Brain route keeps its legacy fallback. Frozen platform-specific broken-output
case is excluded and not counted as passing. Complete#770/#769, independent full
review and native three-OS proof remain required. Windows audit spelling is still
partial; no cross-compile/foreign runtime acceptance is claimed.

Existing target `/workspace/symaira-guard770-diagnostics/target` is released to
root's independent review. Final `/proc` exe/cwd/fd scan found0 users; no build or
replay remains. Use this target without duplicating build outputs. From the
repository root, under `python3 /tmp/symaira-subreaper.py bash -c`, use
`set -euo pipefail`, `umask022`, source `/home/agent/.cargo/env`, explicit
PATH `/workspace/toolchains/go1.26.7/bin:$PATH`, CARGO_INCREMENTAL0 and DEV/TEST
DEBUG0. Run `scripts/guard-standalone-oracle/run.sh REPORT.json`, plus component
Cargo tests and strict clippy. Source and target remain untouched awaiting review.
