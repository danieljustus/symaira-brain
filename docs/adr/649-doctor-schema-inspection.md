# Doctor must inspect real memory schema (#649)

## Decision

Keep schema initialization and atomic, idempotent repair in `symbrain-memory`.
Doctor opens the selected existing database read-only and compares its actual
column names with the schema embedded in the same binary. Expected facts are
created in a private in-memory SQLite connection from `schema::SCHEMA`; applied
migration names are not evidence that the required DDL exists. Doctor reports
missing tables through their missing columns and lists missing `table.column`
values in stable order. It does not invoke `Store::open` or repair during diagnosis.

Use the existing `memory_db.error` field and the human `✗` renderer. Preserve
Doctor's existing report exit policy. For an integrity-valid database missing
the five reported columns, `quick_check` remains `ok` while `error` names the
missing schema. This is an intentional correctness change from frozen Go's
false-green Doctor, rather than an accepted parity claim for the broken store.
Healthy Go-created and repaired stores retain the existing memory health fields.

## Reason

The reported database records every migration but lacks `embedding_binary`,
`embedding_dim`, `embedding_quantization`, `lsh_hash` and
`consolidated_into_id`. SQLite integrity checks cannot prove application schema.
Reading the binary's authoritative schema avoids a second handwritten list that
could drift as memory evolves. Keeping inspection separate from repair makes
Doctor useful before an operator decides to open the store. The existing
IMMEDIATE transaction still owns repair, indexes and migration bookkeeping.
The focused Doctor database module keeps production files below 400 lines.

This check proves required table/column presence. It does not claim complete
column-type, constraint, index, trigger or view equivalence. The separate frozen
DB contract covers those schema facts. A read-only SQLite connection can use
SQLite's WAL sidecar handling; verification establishes that Doctor does not
change the main database contents, not that no filesystem sidecar can exist.

## Verification and acceptance

A supplemental oracle builds immutable Go revision
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` using Go 1.26.7. The real Go CLI
creates a private store. Three process observations cover that healthy store,
the same store after removing the five actual columns while retaining applied
migration records, and the store after native opening repairs it. Full
`memory_db` fields are compared on healthy states; the broken state requires
the documented diagnostic difference and `✗`. Main database hashes must remain
unchanged by diagnosis. Operator HOME, credentials and database are not used.

The unchanged Go executable is also supplied as the purported corrected
candidate: its real false-green output must fail the missing-column assertion.
The original control log is retained. Public Store tests additionally preserve
a Unicode memory row, migration count and repeated-open idempotence; prior
rollback and concurrency tests remain in the Memory suite. All original Go and
historical fixture bytes remain unchanged.

A dedicated native Linux/macOS/Windows workflow runs the schema tests, affected
component checks and the actual Go/native process/control gate, retaining
receipts on failure. Linux working-tree observations are development evidence;
only clean-source proof and fresh native jobs establish acceptance. #649 stays
open until the repaired native store and diagnostic are delivered and their
acceptance is verified. Full Memory CLI writes, HTTP/sync and #758/#759/#762
remain separately tracked.

## Independent view-substitution correction

The initial clean689b1a8 review reproduced a false-green database: replacing
required `sessions` with a view exposing all three column names passed
`PRAGMA table_info`, even though writes failed because it was not a table.
Retain that original process evidence. Inspect the actual `sqlite_schema.type`
as well as required names; a view now reports every required column of the
missing table. This closes the claimed table-presence contract without
broadening the diagnostic into complete DDL equivalence. A same-column view
regression supplements the existing missing-table test.

The supplemental runner also converts its owned native Windows temporary root
into MSYS form before tar extraction. The original Guard Windows failure
establishes why the same Bash/Tar boundary matters; this is a static portability
correction, not a claim that Doctor's Windows runner has been executed locally.
Fresh exact-source proofs, corrected independent review and native CI are required.

## Report actual Windows file attributes

The corrected512 review closes the view finding but identifies a static native
Windows gate blocker in inherited Doctor reporting: Go1.26.7 maps a writable
file to0666 and a readonly file to0444, whereas Rust always reported0600 and
mode_ok=true. Preserve the complete independent review and verbatim pinned SDK
source. Report Go's actual readonly-bit-derived mode and false mode_ok; neither
Windows attribute value proves the POSIX0600 requirement or a private ACL.
Do not project away fields in the comparator or claim an unperformed ACL check.
The supplemental native Windows gate additionally compares a real readonly
attribute fixture and restores it before modifying the owned database. Linux
continues to exercise its three observations; Windows requires four. These
are static Windows changes until the actual native job executes successfully.

Go's Windows Stat mapping also adds0111 for directory metadata, so an owned
writable/readonly directory at default.db is0777/0555 rather than0666/0444.
Preserve this distinction before SQLite rejects the directory. A Windows-only
actual CLI regression checks both attribute states and the absence of a private
ACL claim. It is not executed by local Linux tests; native Windows must run it.
