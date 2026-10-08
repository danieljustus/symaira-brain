# Verify owned Memory defaults before admitting repairs

Status: source-prepared successor; no compiler, candidate runtime or native-OS
acceptance yet. Refs #649/#758. Parent source dba88d23 is retained unchanged.

The independent static review found that dba88's final column verifier compared
defaults only for ALTER-added columns. An original sync_oplog.ts with a missing
or wrong default could therefore be certified complete. Its unchanged owned
memory trigger omits ts: missing DEFAULT rejects the next write, and a wrong
literal records an invalid change timestamp. Three actual owned Python SQLite
3.53.1 cases show the successful timestamp, NOT NULL/1299 failure with unchanged
memory/oplog state, and invalid literal respectively. These are SQL invariants,
not a compiled dba88 or Go constructor claim. The original REQUEST CHANGES
review, receipt and all17 bound independent files are retained before this fix
under migration/evidence/memory-historical-defaults-649/original-dba88 (18 maps
including the receipt; identical SQL files share exact gzip bytes).

The new source-bound SQL inventory constructs only literal trusted owned
schemas. Every one of the 184 Go-owned CREATE/ALTER columns is enumerated,
including its 62 declared defaults and 122 absent defaults. All37 resources
match immutable dcddcef0 bytes. The actual known native6ae footprint differs
at precisely query_log.created_at: CURRENT_TIMESTAMP versus datetime('now').
The current flattened schema equals Go's defaults everywhere. The inventory
retains every raw declaration, SQL input, source hash and three original control
outcomes; a fixed timestamp-pair control records their equal UTC-second values.
The earlier original census/proposal text's SQLite3.40.1 description is retained
as originally written; its actual receipt identifies3.53.1, used in new summaries.

Choose exact default admission for all owned columns, not marker trust. Existing
owned defaults are checked before any row repair or ledger INSERT; every newly
added/created declaration is checked again during step/final verification. The
sole compatibility exception is the known query_log.created_at CURRENT_TIMESTAMP
form. It does not apply to another table's timestamp or an arbitrary expression,
case/whitespace projection, explicit NULL where Go declares no default, or caller
SQL whose value happens to match now. No caller expression is executed to infer
compatibility. A new exception requires preserved actual ownership/effect proof.

An incompatible declared default fails the IMMEDIATE transaction. Preserve the
existing table, rows, custom objects and earlier markers; do not rebuild a table
or change its default blindly. Missing additive columns still take their exact
owned definitions. Required timestamp omission contracts include the oplog ts,
audit/query timestamps and schema ledger timestamp; string/numeric/governance
and empty/absent defaults use the same owner. No new hidden metadata marker or
dependency is introduced. Existing base nullability preservation and all FK,
UNIQUE/CHECK/WITHOUT ROWID/FTS/trigger/UUID eligibility decisions are unchanged.
AUTOINCREMENT declaration certification is not expanded by this focused fix.

Prepared public Store::open tests cover missing/wrong oplog defaults before a
rejecting marker callback or relation repair, original/additive missing/wrong
values, an introduced NULL/function default, and the narrowly compatible old
query timestamp. Each rejection compares all original schema/table/shadow cells.
The compatible case preserves existing rows and DDL, writes a real public memory
with a parseable oplog timestamp, then requires full repeated-open stability.
An additional public-opener loop changes each of the 62 declared omission
contracts independently, requiring the exact default rejection and complete
state preservation. These four new test functions have not executed or compiled
at this checkpoint.

The successor's28 source-only migration test functions (24 parent +4
new), full inherited Store/evidence/CLI/gateway/MCP checks, 37 constructor pairs
and37 reopens/nine controls,590/60/16/13/10 inherited Memory routes and controls,
all concurrent/rollback and prior stdio controls remain required after exclusive
resource allocation. The three-OS workflow includes both compiled component
contracts and the new SQL inventory. Independent runtime review, exact-source
native Linux/macOS/Windows CI and shipped release are still required. No issue
closure, Go fallback removal or release claim follows from this source checkpoint.
WAL-policy prerequisites and wider Brain13 work remain in their separate lanes.
