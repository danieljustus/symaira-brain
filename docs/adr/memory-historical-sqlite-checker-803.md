# Reproducible historical Memory integrity checking

PR803 head `a30bba8bbd52323fa5e000207f1c391d576ac86c` failed Ubuntu job
111389627809 before completing its first historical pair. Both actual product
processes exited zero. The host Python SQLite3.45.1 reported `NULL value in
memories.importance` for both databases, while reading importance as REAL0.5.
This observation remains a failed gate; it is not native parity acceptance.

The complete job log, artifact11296789771, all twelve extracted members and
original source are retained with lossless hash/length/round-trip bindings.
Read-only checks of these exact Go and Rust database bytes reproduce the
failure in the official SQLite3.45.1 amalgamation. Both pass with Python's
SQLite3.46.1 and3.53.1 without modifying either database. A three-statement
owned fixture also reproduces the bug: create a row, add a REAL NOT NULL column
with DEFAULT0.5, then check integrity. The returned value is0.5 although the
old checker reports NULL. The SDK archive and compiled probe are retained;
its sqlite3.c SHA3 matches the official release history. No newer product
process was executed as part of this diagnosis.

Decision: use the repository's already pinned setup-python action and select
Python3.13.7 explicitly for the three native Memory CLI runners. Additionally,
test the actual SQLite engine rather than assuming an interpreter version
implies correct behavior. Before migration replay, a disposable preflight
must accept the historical REAL default, support the existing FTS5 porter
search/integrity contract, and detect a genuinely corrupt NOT NULL declaration
containing NULL. It records Python, executable, SQLite version/source ID,
compile options and the checker source hash. Any failed control aborts; no
SQLite diagnostic is translated into success or omitted from comparisons.

The same preflight actually passed with local SQLite3.46.1 and3.53.1. Using
the official3.45.1 shared library with the system Python made the unchanged
preflight reject the defective checker before producing a success receipt.
These are checker controls, not new Go/Rust migration runs or proof of the
selected Python distribution on macOS/Windows. Fresh integrated-head native
CI must still execute all37 pairs/reopens, nine controls, original Memory CLI
and write/evidence gates. Independent review and protected checks remain
required before merge. The separate Managed timeout coverage failure is not
resolved by this change.

This avoids modifying compatible historical schemas or data to accommodate a
host diagnostic bug. Explicit SDK selection plus executable semantic controls
makes future runner-image changes visible. Evidence is under
`migration/evidence/memory-historical-649/sqlite-checker-a30`.

## Fresh local replay and independent review

The clean64cba289 checker/harness now actually completed all37 original Go/native
pairs,37 complete-state native reopens and nine separate corrective controls
under local SQLite3.53.1. The restored actual63fbd CLI has its original archived
SHA and all549 original Rust/Cargo/SDK/resource inputs matched64cba byte-for-byte.
No target, compiler or embedding port was used. Both products succeed in all37
historical pairs. The separate controls retain nine Go successes and three
native successes/six deliberate fail-closed results, not generalized equality.

A different reviewer verified every222 historical and45 control file hash,
the full receipts/results/source and both product identities; fresh SQLite-only
observations also reject three genuine fixture mutants. Full independent review
has no blocking findings. Its SDK input check preserves the minimal script's
original stdin without final newline and retained file with newline as distinct
byte strings; both reproduce the same old-checker failure. This is not evidence
of the selected Python3.13.7 distribution or native macOS/Windows execution.

The integrated Managed change adds failure-only context to the unchanged Unix
timeout predicates/fixture/budgets. Its separate different-author source review
accepts diagnostic publication only. The final publication retains548 unchanged
original compiled inputs, the one changed Managed test and the additive Windows
health diagnostic as explicitly uncompiled diagnostic tests. It claims no new
Managed runtime result or production fix. Complete originals, local runtime and
both full reviews remain in `current-root-and-independent` beside the original
checker evidence. Fresh native/coverage/protected CI still governs merging.
