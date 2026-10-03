# Preserve raw skills flag diagnostics

Status: accepted implementation decision for #793 / PR #794.

The independent review of `d7d0acda1ec5d07c8cfb18f8b6ba67060a4d608c`
found that unknown flag names and malformed flag spellings were converted
through `to_string_lossy()`. Eight actual process comparisons differed from
the unchanged Go reference, although all 198 existing probes passed.

Decision: match ASCII flags as byte spans and retain the original operand
when reporting undefined names or bad syntax. Unix writes those bytes directly;
Windows uses the UTF-16 replacement behavior of Go's process arguments.
Do not quote these operands: the reference's flag parser emits them unquoted.
A valid U+FFFD remains distinguishable from an invalid byte on Unix.

Rationale: argument validation happens before filesystem access and must retain
the existing CLI contract. Converting a malformed argument to display text
changes that contract and can conceal which bytes were actually received.
This correction does not broaden the known flag vocabulary or native fallback
eligibility. Whole-vector normalization, equals splitting, help, missing-value
errors, scope/target validation and no-write behavior retain their contracts.

The original independent review and failed observations are retained under
`migration/evidence/skills-preflight-793/`. Native process regressions cover
literal Unicode, invalid sequences, inline values, bad syntax, normalization,
and valid flags before the unknown operand. Fresh native CI and independent
review of the corrected revision remain required before merge.

Native Windows CI on published head `6fff6058` rejected an unconditionally
imported Unix-only test helper under strict Clippy. Its import now has the same
`cfg(unix)` boundary as its callers; production behavior is unchanged. The
failure remains recorded with both job IDs in the adjacent evidence receipt.
The corrected published head must pass fresh native checks before acceptance.

After Activity PR800, normal-integrate main31de722 and repeat the complete
315 actual raw-argument Go/native pairs on the resulting CLI. All agree.
The fresh skills/CLI suite has409 primary tests,0 failures,2 intentional
child-entry ignores and54 unfiltered summaries, with strict Clippy and fmt
passing. Historical446 belongs to its earlier source, not this run. This
checks the current combination while preserving earlier review/CI evidence;
fresh native three-OS acceptance still gates merge and #793 completion.
