# Finish historical Memory migrations through their actual effects

Status: census and implementation proposal; no production change or approval.
Canonical Store source: `6ae73a7eef3442e2b4211a56043941339fb5a1d6`, normal main2b.
Read-only Doctor combination: `cd9cefa5a2014efe40679d411a3cbd491a9aca65`.
Frozen Go: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` / CoreKit v0.17.0.
Refs #649 and #758. This proposal belongs in a new isolated Store successor;
existing published/reviewed worktrees, production Go and frozen fixtures stay
unchanged. The coordinator allocates the source/target scope after this census.

## Evidence and the remaining problem

37 ordered historical prefixes were created from literal frozen Go SQL, with
recorded migration ledgers and meaningful original-column rows. The actual
immutable Go CLI and an archived native CLI opened independent owned copies.
Go completed all37; native completed35 and failed after020/021 because its
relation-ID index precedes the missing022 columns. Public native `Store::open`
is actually reached by this CLI's list command. Migration/schema/constructor
source bytes at the archived7ca CLI are identical to canonical6ae and combined
cd9; this is constructor evidence, not a claim that their entire CLI binaries
or current write routes are identical. No new build, target, embedding endpoint,
operator HOME/credential or external service was used.

| Effect | Actual remaining difference | Required successor behavior |
| --- | --- | --- |
| 006 provenance | Allfive prefixes before006 leave rule updated_at NULL; Go copies created_at | Backfill eligible legacy rows after column repair; preserve existing timestamps |
| 022 relation provenance | Both020/021 fail native open; Go generates two distinct UUIDv4 IDs and copies relation created_at to updated_at | Repair all relation columns before UUID/timestamp backfill and unique-index creation; never regenerate nonempty IDs or modify entity IDs |
| 025 temporal relations | Successful022/023/024 native opens claim025 with neither valid_from nor valid_until | Add both nullable fields and temporal indexes; retain NULL open-ended semantics, without invented dates |
| 029 query attribution | Successful query-log historical states lack actor/scope/session despite claimed029 | Repair these columns and actor index without changing previous log rows |
| 030 audit targets | Historical audit tables lack target_type/target_id despite claimed030 | Repair both nullable columns and target index without changing previous audit rows |
| 016/027 FTS | 14 pre016 states get an empty native external-content index and fail rank1 integrity; 25 successful states lack Go's actual run stem hit | Build/rebuild existing memory content when first creating FTS; upgrade the known old unicode61 owner to porter unicode61 with all three exact maintenance triggers |
| 034 activity Store | 15 states retain45 old unconditional oplog triggers while claiming034 | Replace only the known owned insert/update/delete triggers with Go's sync_exclude predicates |
| Owned indexes | 21 Go index names are absent in successful native states, including the unique relation-ID constraint | Include every Go-owned index/uniqueness fact, with repair/data-before-index ordering |

Two additional bounded SQL invariant pairs on copies of the actual post-open
states show practical effects: duplicate relation IDs are accepted by native but
rejected by Go's unique index; a sync_exclude=true insertion makes one native
oplog event and none under Go's upgraded trigger. These invariant/FTS queries
use Python SQLite3.40.1 to inspect/operate the actual Go/native migrated files;
they are not compiled Go relation/retrieval API calls. The migration executions
are actual Go modernc and native bundled SQLite. The permanent successor gate
should also execute these constraints inside the respective compiled owners.

Eight actual second opens preserve all application state at four selected
prefixes. Stable native state remains incomplete: its blanket marker insertion
makes errors durable. List output success and integrity_check=ok alone cannot
establish completion or FTS external-content correctness.

One schema default metadata difference is retained: fresh native query_log
created_at reports CURRENT_TIMESTAMP, Go reports datetime('now'). Both produce
UTC second timestamps; changing a populated table solely for equivalent SQL
spelling is unnecessary unless an actual consumer requires that metadata.
Existing fixtures show Go changing the owned0644 database to0600 while native
leaves0644. The fixture directories already existed; this is no fresh-directory
or native Windows mode proof. Shipped repair acceptance must include existing
file safety/modes; the existing broad file-mode cutover gate stays truthful.

The initial recipe attempted two blank relation IDs after022 before assigning
IDs, violating Go's real unique index. That aborted after20 pairs; its exact
script, traceback, partial directories/results remain. The outer inspection
command's trailing tail exit is not a passing complete census. A separate37-
prefix recipe inserts valid IDs at creation after022. No original result was
rewritten or reclassified. See migration-inventory.json/.csv, successful-
structural-gaps.json, historical-trigger-gaps.json and literal result.json files.

## Proposed long-term implementation

Keep the Store constructor as the single repair owner. Doctor stays a read-only
consumer of the binary-owned schema; it must not acquire mutation/repair code.
Keep UI/HTTP admission, MCP eager JWT/audit startup, configuration/credential
resolution and transport work in their separately assigned scopes.

Keep the approved bounded busy_timeout and IMMEDIATE transaction. WAL/secure-
delete/foreign-key pragmas stay in their required pre-transaction order. Reserve
the writer before any schema facts to avoid the independently demonstrated
BUSY_SNAPSHOT regression. Commit repaired DDL, data, indexes/triggers and ledger
facts together; any later error must leave application state/ledger unchanged.
Go commits individual migration files; Rust's stronger atomic repair is already
an explicit approved decision, so document that failure-state distinction.

Replace unconditional all-version bookkeeping with verified ordered migration
postconditions. Preserve the 37 exact identifiers, including all five distinct
027 filenames and omitted013. Take migration eligibility/facts before repair;
additive partial shapes need per-column checks, never broad SQLite error-string
suppression. Do not replay already-present ALTER statements, trust a forged
marker, invent absent original content/scope fields, or silently treat a view as
a writable table. Unknown/corrupt base objects should fail with full rollback.

Use small local Rust modules/SQL resources for owned schema facts, additive
repair, eligible data backfills and FTS/trigger transitions. Copy required frozen
DDL into new Rust-owned resources with exact provenance; do not edit Go or make
Rust execution depend on the future-preserved/removed Go production directory.
No new CoreKit layer or dependency is justified for this one Store consumer.

The two genuine row backfills are006 and022. Null updated_at eligibility must
reflect pending migration or a newly recovered column; a current legitimately
nullable field must not be rewritten just because it is NULL. Existing native
false-completion states need an explicit corrective postcondition policy before
implementation, with matching positive/preservation fixtures. Relation blank
IDs require bound UUIDv4 uniqueness before creating the unique index; retain
existing IDs, evidence, provenance, entity primary keys and relationship triples.
025 only adds nullable interval bounds/indexes, with no row backfill. No attempt
should assign temporal validity to old relations.

FTS repair must establish external-content/index consistency, not only table or
column presence. Preserve memory rowids/content and rebuild only the known
owned missing/legacy/stale FTS shape, with triggers and ledger in the same
transaction. Existing correct porter indexes must remain stable across reopen.
Do not drop an unknown user-defined virtual table or treat a parser/string match
as proof of tokenizer semantics. Define and test how old falsely marked native
empty porter indexes are recognized and repaired; account for large-store scan
cost rather than rebuilding every open without evidence. Do not conflate this
with hybrid ranking/candidate parity in #758.

## Acceptance work before release/closure

1. Keep all37 prefix fixtures and their original current-vs-Go receipts. Execute
   actual Go/native constructors on every prefix, and compare every table's
   columns/type/nullability/default/PK, foreign keys, owned indexes/uniqueness,
   trigger behavior and ledger postconditions. Cover fresh and in-memory stores.
2. Bind generated relation IDs to unchanged original triples, check UUIDv4 shape,
   uniqueness and timestamp preservation; preserve nonempty IDs/timestamps and
   all unrelated rows/blobs. Cover eligible NULLs separately from post-migration
   NULLs and invalid types/foreign references. Exercise real unique constraints.
3. Add declared/pending/mixed/lying ledgers, each five #649 columns independently
   and together, partial022/025/029/030 shapes and missing/old/correct/stale FTS.
   These damaged-ledger fixtures are corrective native decisions: frozen Go may
   skip or fail them and must remain honestly labeled as such.
4. Verify actual porter stems from preexisting and newly inserted rows, update,
   delete, external-content integrity, rowids and all unrelated state. Check all
   three known oplog trigger predicates with sync-excluded and ordinary rows.
5. Use actual aborting backfill/ledger triggers or a later owned-index failure to
   prove rollback through columns, data, dropped/recreated FTS/triggers, indexes
   and markers. Keep PRAGMA/journal/file-metadata effects outside an unsupported
   byte-for-byte database rollback claim. Preserve existing concurrency proof.
6. Repeat corrected public opens with all application-state/FTS shadow bytes and
   IDs stable. Run inherited Store/CLI/gateway/HTTP consumers at the agreed scope
   and exclusive endpoints, without broadening native routes or adding keys.
7. Preserve fresh meaningful failure controls for a suppressed backfill, false
   marker, omitted FTS rebuild and missing unique/trigger effect. Run native
   Linux/macOS/Windows and required CI plus different-author full review, then
   verify the shipped Rust release/repro before #649/#758 closure.

## Ledger precision

DB-004 is a proven five-column repair/concurrency/rollback slice, not complete
historical migrations. Keep it and DB-005's read-only diagnosis truthful. Add a
historical-repair row for all37 migration effects, pending until the successor's
actual/native-three-OS/release evidence exists. DB-001 currently asserts table,
memories columns/indexes, trigger/view and pragmas from a fresh frozen fixture;
its index_facts helper inspects only memories. Clarify that existing scope; do
not silently relabel it all-table historical/index repair or rewrite fixtures.
#649 stays open until shipped repair, and #758's remaining retrieval/admission/
serve/sync/other verb work remains separate. No complete issue closure is made
by this read-only preparation.
