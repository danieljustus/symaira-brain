# ADR 0006: Preserve activity integer parser error precedence

Status: accepted implementation decision; PR #800 acceptance remains subject to an independent replacement-head review and native CI.

## Decision

The native activity CLI scans integer arguments as bytes, matching Go 1.26.7's base-zero `ParseInt` ordering. It accumulates the unsigned 64-bit magnitude while skipping separators, checks separator placement only after accumulation succeeds, and checks signed range last. Invalid UTF-8 remains an ordinary byte-level parse error at the position where it is encountered; it is not validated before scanning begins. Binary, explicit and legacy octal, hexadecimal, signs and prefix separators retain their existing semantics.

## Reason

The independent review of `36ccc6ef60815806d38bfbec4ddbf9e96679d648` found three actual Go/Rust process mismatches. For example, `1__9999999999999999999999999999` reports `value out of range` in Go but reported `parse error` in Rust. Whole-value UTF-8 validation also concealed an earlier unsigned overflow. These errors are observable command contracts even though all affected requests safely exit 2 before database work.

Preserving the source-bound ordering lets the activity validation cutover remove its Go fallback without silently changing diagnostics. A reusable byte parser is simpler to maintain than exceptional fallback branches for each malformed input combination. The fixed Go oracle and comparisons remain unchanged; supplemental actual-process cases cover both activity integer flags, all supported bases, signed boundaries, misplaced separators and raw-byte error precedence.

## Verification and scope

The original independent review and actual mismatches are retained in `migration/evidence/activity-validation-767/independent-review-36ccc6.md` and `independent-numeric-precedence-36ccc6.json`. The expanded suite contains 237 portable cases plus 28 Unix raw-argv cases. Missing fixtures, changed exits and removed cases must still fail the actual native fixture test. Three-OS exact-head CI and an independent review of the replacement implementation remain merge conditions.

This decision concerns ACT-CLI validation and diagnostics. It does not close the memory/importer contracts in #758/#761, change the immutable Go baseline, broaden profile access, or alter database behavior.
