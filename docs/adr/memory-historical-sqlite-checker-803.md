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
