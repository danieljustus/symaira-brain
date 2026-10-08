# Preserve oracle criteria while repairing PR804 test lints

Status: source-only successor of reviewed `6572c11e34b0540b7cbc5b4ee6fedb9753ac23ed`;
different-author review and fresh actual CI are still required.

Actual Linux job `111466802888` and Windows job `111466802991` in run
`37212657293` now report three test-only strict Clippy failures: two constant
`chunks_exact(2)` calls and a 101-line `compare_with_count` function. The complete
original logs and JSON envelopes, the full prior independent review and all
2,038 of its proof bindings were retained before edits. The original657
worktree and earlier source/evidence checkpoints remain unchanged. These logs
do not establish numerical or native-runtime acceptance.

Use `as_chunks::<2>().0.iter()` for both hex decoders. It exposes the same
complete two-byte groups and leaves the same final remainder unused. Keep the
existing even-length assertion in the generic decoder; introduce no admission
restriction in the SDK regression decoder. The radix conversion, UTF-8 checks,
IDs, expected outputs and all comparisons stay unchanged. This applies the
idiomatic fixed-array iterator requested by current Clippy without a lint
suppression or numeric algorithm change.

Extract the status oracle's per-row environment setup into a small test helper.
Its call stays exactly before the home filesystem snapshot and provider
construction. Keep every environment name/value, full report/request comparison,
case count, identity exception, filesystem assertion and output write in its
original order. Inlining the helper restores the entire original file byte for
byte. This gives the fixture preparation one clear owner and keeps the main
comparison readable without suppressing the function-size lint.

Three exact source projections verify those edits. Four negative source
controls reject a wrong chunk width, removed even-length assertion, moved
environment setup and omitted environment key. Another 337 pure byte-partition
projections cover lengths 0 through 255 and all 81 retained SDK reference rows.
These are source checks, not compiled Rust or Go parsing observations. All
4,747 other original Git bodies, including production arithmetic, all frozen
Go sources, transport controls, fixtures and workflows, retain their hashes and
modes. Rust formatting and workflow lint pass; no Clippy/compiler/SDK/product
process, target, cache, HTTP peer or port was allocated or accessed.

The prior production-lint and formatter-ownership decisions remain intact.
Require full different-author review before ordinary push for fresh CI. Require
actual strict affected/full Rust tests, both real-Go formatter regressions and
`make lint`, the six original Usage gates, original Retry1600/1164 and additive
1681/1356 with all seven actual controls, and exact-source Linux/macOS/Windows
acceptance before merge or release. Historical or source-only approvals do not
replace those gates. PR804 and issue #768 remain open.
