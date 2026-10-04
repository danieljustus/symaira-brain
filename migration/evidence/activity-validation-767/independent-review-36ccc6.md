# Independent review of PR #800

Reviewer: independent read-only Codex agent `review_pr800`.
Base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
Reviewed head: `36ccc6ef60815806d38bfbec4ddbf9e96679d648`.
Worktree: `/workspace/symaira-activity767`, clean at inspection and after execution.
Disposition: **changes requested**, one reproduced P2 correctness finding.

## P2: Preserve Go integer error precedence before removing fallback

Location: `rust/symbrain-cli/src/activity_args.rs:177` (whole-value UTF-8 validation) and `:39` (eager underscore-placement validation).

For an explicitly granting activity profile, `activity status --profile=activity-oracle --max-tokens=1__9999999999999999999999999999` emits `value out of range` in the frozen Go CLI, but `parse error` in this candidate. Both processes exit 2 with empty stdout, so the regression is in the externally visible stderr contract. The same mismatch occurs with `0x__ffffffffffffffffffff` and, on Unix, a decimal uint64 overflow followed by a raw `ff` byte. This becomes native behavior when the PR removes the fallback formerly responsible for unsupported/invalid flag shapes.

Go's base-zero `strconv.ParseUint` scans bytes, skips underscore bytes during accumulation, and returns unsigned overflow immediately. It validates underscore placement only after accumulation succeeds; an invalid later byte cannot override an earlier overflow. The candidate checks the whole argument as UTF-8 before scanning and rejects duplicate underscores while scanning. Parse the original argument bytes and defer underscore-placement checks to reproduce that ordering, while retaining signed-range validation after unsigned parsing. Add actual Go/Rust cases for ordinary invalid underscore sequences, overflowing invalid underscore sequences, overflow followed by malformed UTF-8, and signed boundaries. Do not change the frozen Go implementation or weaken comparisons.

Reproduction data: `/tmp/symaira-pr800-independent-numeric-observations.json`. The five independent probes contain three mismatches and two matching overflow controls. Go was freshly built from immutable `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` by `scripts/run-go-oracle.sh`, using Go 1.26.7 and `CGO_ENABLED=0`. Go SHA256: `f09ab64c36c9458b031a65baa528e30402291528036019308706ce070db7de67`. Native SHA256: `ee93c828b22931459d84467502c1ec58faeeedb443e60b4b63c67403ef020dae`.

## Scope and inspected files

Read root `AGENTS.md`, the complete immutable diff, all changed production files (`activity_args.rs`, `activity_time.rs`, `activity_cli.rs`, the router change in `symbrain-cli/src/lib.rs`, and `symbrain-activity/src/lib.rs`), moved activity unit tests, both CLI integration test files, broker marker-readiness test change, three activity runner/controls scripts, workflow additions, README, migration narrative and contract rows. Checked frozen Go activity command/profile scanning, flag normalization, activity validation ordering, and the Go 1.26.7 integer parser source. Inspected the retained validation fixture, process receipt source hashes and validation metadata, and preserved failure-evidence structure.

Reviewed correctness, bounded work, policy gating, stdin/stdout behavior, portability and fallback removal. The CLI still checks the explicitly granting profile before parsing invalid requests, preserves Go's long-profile scanning/last-occurrence precedence, and validates bounded search inputs before database access. Whole-vector flag normalization, positional/terminator behavior, raw-byte flag diagnostics, per-byte malformed UTF-8 query rune counting, zero-time validation, and the sampled RFC3339Nano diagnostics matched their source contract. No new network call, credential exposure, authority expansion, unsafe code, or unbounded subprocess was introduced by these production changes. The broker change waits for a complete newline-terminated numeric PID record before shutdown assertions; it preserves the descendant/process-group assertions and fixes an actual producer-readiness race.

The read/store implementation is largely existing code; this review does not certify unported dynamic memory configuration/importer contracts covered by #758/#761. Existing oversized unrelated files were not edited by this candidate. Changed production modules remain below 400 lines.

## Independently executed verification

* Verified every candidate source hash in the tracked clean-source receipt (`bdc6d30e4d31da0fb3dda90e6bad5d16850e6285`) matches the reviewed head; verified existing native executable SHA256 matches that receipt exactly.
* Built a fresh Go CLI from the immutable oracle revision, without editing the oracle or production tree.
* Replayed the full 185 actual Go/native activity cases with absent fallback and disposable HOME/XDG roots: **185/185 match**. Receipt: `/tmp/symaira-pr800-independent-baseline-185.json`, including reviewed head, clean status, source and binary hashes.
* Executed 24 additional RFC3339 malformed date/offset, control-byte, nonstandard accepted-hour and range-precedence probes against that fresh oracle: **24/24 match**. Receipt: `/tmp/symaira-pr800-independent-time-observations.json`.
* Executed five additional numeric precedence probes: **three mismatches reproduced**, two matching controls, as described above.

All subprocess verification used the session subreaper, `set -euo pipefail` and `umask 022`. No source edit, commit, push, PR state change, review submission or merge was performed. Existing broad test/stress and failure-control receipts were inspected, not falsely claimed as independently rerun. Native macOS/Windows acceptance and protected GitHub checks remain the root agent's exact-head merge gate; this Linux review cannot substitute for them.

## Non-blocking cleanup

`README.md:661` starts the added sentence with `#Activity` without a space. It renders as an accidental literal hash rather than a useful heading or prose; remove the hash and retain the existing Development heading level.

After the P2 fix, independently review the replacement immutable head and rerun the added actual Go cases before approving the fallback cutover.
