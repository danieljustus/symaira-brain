# Skills strict lint admission after the first native failure

The first macOS source checkpoint recorded 23 strict pedantic Clippy
diagnostics in `symbrain-skills`. Its original raw log and source snapshot
remain in the preserved source checkpoint and are not part of the PR. The
correction was authored against immutable
`b49372bedcd9c4514706c999447dc4f836be012a`. It is integrated with current
main here; new strict Clippy results are reported separately from the
historical source-only checks.

We fix warning owners where doing so preserves their contracts. A bundle
load binding is renamed; an unused import is removed; the comment on an
`include!` becomes a normal comment; documentation uses code spans and states
its existing panic conditions. Explicit map defaults, an unnecessary borrow
and a trailing format comma are cleaned up. Context iteration enumerates the
same slice from the same index, preserving first-match order. Variant dispatch
uses `if let` with the same UTF-8 success and failure bodies, including all
raw-byte returns and marker errors.

The bisect grammar width takes only values 1 and 4. Giving it a byte type and
using widening conversions eliminates a hypothetical truncation without
changing any trie, pattern, EOF, integer or error semantics. The Unix path
conversion takes the low byte with `to_le_bytes()[0]`: that equals the previous
`u16 as u8` for every possible unit, including values outside the actual Unix
byte domain. It adds no panic and does not normalize bytes or Windows units.

Three specific lint exceptions retain established contracts. The public
installer's independent booleans remain source-compatible rather than being
replaced by a new options API. The bounded newline-count statement retains
its byte scan and avoids a new dependency. Only the archive extractor retains
`Debug` formatting: changing it to `Display` would remove quotes and escaping
from existing path diagnostics. Its two format arguments are inlined while
keeping their exact `:?` conversion. No crate-wide lint rule changes.

The 17-file source correction above describes the preserved
`74a6b0549250d2c7c3acae903b94bafb34212c2f` lint delta against
`b49372bedcd9c4514706c999447dc4f836be012a`; it is not a claim that this
integrated branch differs from the PR source only in those files. This branch
also reconciles current-main shared CI/matrix/CLI/core/sync contracts and
dependency edges; the resolved lockfile retains the same package/version
identities.

The full-workspace strict lint run also found current-main sites in
`go_json_float_tests.rs`, `raw_skills` source/tests, and the integrated CLI
and gateway. Fixed-size byte-pair iteration and raw-argv byte copies preserve
fixture bytes; the shared byte module is now loaded once. The CLI dispatcher
returns the exit code directly, and post-restore validation/sync was split
into helpers without changing error strings or operation order. Documentation,
constructor-order, raw-string, and character-pattern cleanups preserve request
behavior.

The Windows-only follow-up is limited to `path.rs`, `windows.rs`, and
`windows_lower.rs`: numeric formatting preserves every table value; signed
mapping is checked before conversion; prefix matching remains ASCII-only;
UTF-16-to-byte conversion rejects out-of-range units; and option/ownership
helpers satisfy pedantic lints without changing defaults. No global lint
threshold or allowance is changed. Historical source-map captures remain
historical and were not regenerated to conceal these edits.

The preserved source-only checks do not establish native acceptance.

The recovered working tree passed host `cargo test --workspace --locked`
with 1039 passed, zero failed and two existing ignored tests, strict workspace
all-target/all-feature Clippy and formatting. The remaining MCP assertion was
an incorrect feature assumption, not a surrogate-admission failure:
`symaira-core-llm` enables `serde_json/arbitrary_precision` only in the
workspace build, so `1e9999` already passes the original non-Skills Value
validation there. The regression now verifies that other owners never use
the Skills bypass and retain the original Value validation and exact errors
in both transports, with focused and workspace executions. No production
parser behavior, frozen Go input or comparator was changed for that correction.

Fresh immutable-head review, source-bound Go-oracle execution and native
three-platform gates remain required. A Windows cross-target Clippy check
does not establish native Windows execution or ownership behavior.
