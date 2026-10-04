# Repair historical Memory migrations through their owned effects

Status: implementation decision; source preparation only, no executed validation.
Refs #649/#758. Base PR803 `6ae73a7`, with normal main2b already an ancestor.
The new isolated successor changes only the Store/schema/migration owner.
Doctor stays read-only; UI/catalog/JWT/configuration ownership stays separate.

All 314 original census files, the initial failed 20-pair recipe, the corrected 37
actual Go/native pairs, eight repeated opens and exact executable/source bindings
are preserved under migration/evidence/memory-historical-649/census-6ae. Three
constructor files are byte-identical at archived 7ca/canonical 6ae/combined cd9;
the restored CLI is 7ca, not a newly built6ae binary. SQL inspection/invariant
probes use Python SQLite on actual migrated files, not compiled Go domain APIs.

The census proves missing 006 rule timestamps, 022 relation provenance, 025
nullable interval columns, 029 query attribution, 030 audit targets, 21 owned
indexes, missing/stale preexisting-content FTS and 034 sync-exclusion triggers.
Go completes all 37 ordered historical prefixes; native fails 020/021 and falsely
records later effects. Stable second opens do not establish historical repair.

Keep the existing IMMEDIATE writer reservation, bounded busy timeout and whole-
repair transaction. WAL/foreign-key/secure-delete pragmas retain their required
order outside it. DDL, eligible data repairs, owned indexes/triggers and ledger
records commit together. Go's per-file commits and the native stronger rollback
remain an explicit distinction. No SQLite error-string suppression is allowed.

Keep the 37 exact migration identifiers and ordered Rust-owned copies of their
frozen SQL, with source hashes. Inspect/add each missing additive column; verify
owned table/column/index/trigger postconditions before reporting completion.
The final checks include foreign keys, unique-key collations, CHECK expressions
and WITHOUT ROWID semantics. A matching column list cannot certify a weakened
table constraint. Every new ledger entry is read back before advancing.
Indexes must preserve Go's uniqueness/columns, not only a matching name. Existing
unknown views, mismatched owned indexes or incompatible columns fail with
rollback; do not rebuild user tables or erase custom objects to make them pass.
Additional unrelated columns/indexes/triggers are retained.

An UPDATE that returns success after a RAISE(IGNORE) callback is not sufficient
evidence for corrective repair. Eligible non-NULL source timestamps must no
longer have missing destination timestamps, and blank relation IDs must be
gone. Otherwise abort the whole repair. This deliberately stronger corrective
postcondition differs from Go's migration marker shortcut; it never rewrites
the callback or commits a false completion entry. Original NULL source values
remain NULL rather than receiving invented timestamps.

Only known data repairs are permitted:

-006 copies created_at to NULL rule updated_at when pending, when that column
 was recovered, or for the recognized old-native false-completion signature.
 The signature requires all 37 applied names, the exact old native nonunique
 idx_entity_relations_id definition, and absence of Go's unique relation-ID
 index. It identifies the established native footprint rather than guessing
 row ages. A healthy/current Go store with a legitimate NULL is unchanged.
 Ambiguous claimed-complete NULLs without this evidence are preserved.
-022 gives blank relation IDs distinct UUIDv4 values without changing nonempty
 IDs, entity identities or relationship triples. NULL updated_at is recovered
 when pending/newly added/recognized false completion; otherwise only the rows
 whose blank ID proves incomplete provenance are eligible. Preserve unrelated
 current NULLs and existing timestamps. Apply ID generation before timestamps
 in Go's order; failure rolls back both.
-025 retains NULL valid_from/valid_until open-ended semantics. Other legitimate
 nullable fields (audit/query attribution, access/expiry/retirement and temporal
 bounds) stay NULL; they are not timestamp backfill candidates.

FTS may be replaced only when its stored definition matches the known owned
Go/Rust external-content FTS5 shape (old default/explicit unicode61 or current
porter unicode61). Validate all three named maintenance triggers against known
owned definitions before replacement. Missing known-owned FTS is created; old
unicode61 or a pending 027 transition is rebuilt from unchanged memories/rowids.
A claimed-current porter index gets rank1 external-content integrity inspection;
only SQLite's typed corruption result permits rebuilding that known owner.
Other errors propagate. Unknown tokenizers/content models/plain tables/views or
custom definitions under owned trigger names are never dropped. Additional
unrelated custom triggers remain intact.

Choose correctness over falsely trusting old markers: rank1 consistency is
checked on open, including old falsely marked empty porter indexes. This can
scan a large index while holding the writer reservation; document and measure
that cost once resources are released. Do not add a hidden metadata marker or
rebuild every healthy index. Correct porter/index state and all shadow bytes
must remain stable on repeated opens. Optimize only with equivalent evidence.

034 replaces only the known old unconditional oplog triggers with the frozen
sync_exclude predicates; known current triggers stay stable. Unknown custom
same-name definitions fail instead of being erased. The original plaintext
SQLite rows, not CLI success or quick_check alone, bind acceptance.

Existing database-file mode parity belongs to Store::open: on Unix successful
opens mirror Go's 0600 main-file setting and best-effort WAL/SHM tightening.
Like Go, chmod runs after the migration commit. A main-file chmod failure is
reported with completed migration data retained; file-mode failure is not
described as an atomic database rollback.
New directory safety/modes, native Windows/macOS filesystem semantics and wider
CLI path/configuration cutover retain their separate gates. No operator data
is touched during authoring or tests: only explicitly owned fixtures are used.

Preserve equivalent old query_log default spelling instead of rebuilding a
populated table merely for CURRENT_TIMESTAMP versus datetime('now'). New schema
uses the frozen default. Preserve existing compatible base-column defaults and
nullable legacy declarations; verify added migration-column declarations and
required types/keys/foreign references. No invented missing original data.

Acceptance needs every 37 prefix, fresh/in-memory/damaged-ledger/partial-column
cases, bound UUID/timestamps, real unique/trigger/FTS effects, actual later-stage
rollback and concurrency, healthy NULL/ID/data preservation, inherited consumers
and meaningful marker/backfill/FTS/index/trigger controls. Original frozen
fixtures stay unchanged. Native-three-OS/required CI, different-author review
and the shipped Rust release/repro are required before #649/#758 closure. Builds,
runtime/target/port access wait for explicit exclusive coordinator allocation.
